//! OpenAI-compatible chat-completions adapter.
//!
//! One adapter serves OpenAI, DeepSeek, Ollama, vLLM, LM Studio and any endpoint
//! that speaks `POST {base_url}/chat/completions`. Different vendors are just
//! different [`ProviderConfig::base_url`] / model ids.
//!
//! Endpoints used:
//!
//! * `POST {base_url}/chat/completions` (non-streaming and `stream: true`)
//! * authentication: `Authorization: Bearer <api_key>`
//! * streaming: `text/event-stream`, `data: {...}` frames terminated by
//!   `data: [DONE]`

use reqwest::header::{HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::{Client, RequestBuilder};
use serde_json::{json, Map, Value};
use tokio::sync::mpsc;

use crate::error::{CoreError, CoreResult};
use crate::provider::adapter::{AdapterCall, BoxFuture, EmbeddingCall, ProviderAdapter};
use crate::provider::config::{ProviderConfig, ProviderId, ProviderProtocol};
use crate::provider::http::{self, SseDecoder};
use crate::provider::response::{
    AiResponse, AiStream, EmbeddingResponse, FinishReason, StreamEvent, ToolCall, TransportStats,
    UsageReport,
};
use crate::provider::types::{ContentPart, Message, ResponseFormat, Role};

/// Path appended to `base_url` for chat completions.
const CHAT_PATH: &str = "chat/completions";
/// Path appended to `base_url` for embeddings.
const EMBEDDINGS_PATH: &str = "embeddings";
/// Capacity of the channel between the HTTP reader task and the consumer.
const STREAM_CHANNEL_CAPACITY: usize = 32;

/// Adapter for the OpenAI chat-completions wire format.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleAdapter {
    client: Client,
}

impl OpenAiCompatibleAdapter {
    /// Creates an adapter with its own HTTP client.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Internal`] if the TLS/HTTP client cannot be built.
    pub fn new() -> CoreResult<Self> {
        Ok(Self {
            client: http::build_client()?,
        })
    }

    pub(crate) fn from_client(client: Client) -> Self {
        Self { client }
    }

    fn encode_body(call: &AdapterCall<'_>, stream: bool) -> Value {
        let mut body = Map::new();
        body.insert("model".to_owned(), json!(call.model.id));
        body.insert(
            "messages".to_owned(),
            Value::Array(
                call.request
                    .messages()
                    .iter()
                    .map(message_payload)
                    .collect(),
            ),
        );
        body.insert("stream".to_owned(), json!(stream));

        if let Some(temperature) = call.request.temperature() {
            if let Some(number) = serde_json::Number::from_f64(f64::from(temperature)) {
                body.insert("temperature".to_owned(), Value::Number(number));
            }
        }
        if let Some(top_p) = call.request.top_p() {
            if let Some(number) = serde_json::Number::from_f64(f64::from(top_p)) {
                body.insert("top_p".to_owned(), Value::Number(number));
            }
        }
        if let Some(tokens) = call.request.max_output_tokens() {
            body.insert("max_tokens".to_owned(), json!(tokens));
        }
        if !call.request.stop().is_empty() {
            body.insert("stop".to_owned(), json!(call.request.stop()));
        }
        if !call.request.tools().is_empty() {
            body.insert(
                "tools".to_owned(),
                Value::Array(
                    call.request
                        .tools()
                        .iter()
                        .map(|tool| {
                            json!({
                                "type": "function",
                                "function": {
                                    "name": tool.name(),
                                    "description": tool.description(),
                                    "parameters": tool.parameters(),
                                }
                            })
                        })
                        .collect(),
                ),
            );
        }
        match call.request.response_format() {
            ResponseFormat::Text => {}
            ResponseFormat::JsonObject => {
                body.insert("response_format".to_owned(), json!({"type": "json_object"}));
            }
            ResponseFormat::JsonSchema { name, schema } => {
                body.insert(
                    "response_format".to_owned(),
                    json!({
                        "type": "json_schema",
                        "json_schema": {"name": name, "schema": schema, "strict": true}
                    }),
                );
            }
        }

        for (key, value) in call
            .provider
            .extra_body
            .iter()
            .chain(call.request.extra_body().iter())
        {
            body.insert(key.clone(), value.clone());
        }
        Value::Object(body)
    }

    fn build_request(
        &self,
        call: &AdapterCall<'_>,
        stream: bool,
    ) -> CoreResult<(RequestBuilder, u64)> {
        let body = Self::encode_body(call, stream);
        let payload = serde_json::to_vec(&body).map_err(|error| {
            CoreError::serialization_with_source(
                "failed to encode the OpenAI-compatible request body",
                error,
            )
        })?;
        let request_bytes = u64::try_from(payload.len()).unwrap_or(u64::MAX);

        let accept = if stream {
            "text/event-stream"
        } else {
            "application/json"
        };
        let mut builder = self
            .client
            .post(call.provider.endpoint(CHAT_PATH))
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, accept)
            .body(payload);

        builder = authorize(builder, call.provider)?;
        Ok((builder, request_bytes))
    }
}

/// Builds the `POST {base_url}/embeddings` body.
fn encode_embedding_body(call: &EmbeddingCall<'_>) -> Value {
    let mut body = Map::new();
    body.insert("model".to_owned(), json!(call.model.id));
    body.insert("input".to_owned(), json!(call.request.inputs()));
    // Float arrays are the only format every compatible vendor returns as JSON.
    body.insert("encoding_format".to_owned(), json!("float"));
    if let Some(dimensions) = call.request.dimensions() {
        body.insert("dimensions".to_owned(), json!(dimensions));
    }
    for (key, value) in &call.provider.extra_body {
        body.insert(key.clone(), value.clone());
    }
    Value::Object(body)
}

/// Parses `{"data":[{"embedding":[...]}],"usage":{...}}`.
fn parse_embedding_response(
    provider: &ProviderId,
    model: &str,
    value: &Value,
    transport: TransportStats,
) -> CoreResult<EmbeddingResponse> {
    let data = value.get("data").and_then(Value::as_array).ok_or_else(|| {
        CoreError::provider(
            provider.to_string(),
            format!("provider=`{provider}` model=`{model}` returned no embedding data"),
            false,
        )
    })?;

    let mut vectors = Vec::with_capacity(data.len());
    for (index, item) in data.iter().enumerate() {
        let components = item
            .get("embedding")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                CoreError::provider(
                    provider.to_string(),
                    format!(
                    "provider=`{provider}` model=`{model}` returned no vector for embedding {index}"
                ),
                    false,
                )
            })?;
        let mut vector = Vec::with_capacity(components.len());
        for (position, component) in components.iter().enumerate() {
            let number = component.as_f64().ok_or_else(|| {
                CoreError::provider(
                    provider.to_string(),
                    format!(
                        "provider=`{provider}` model=`{model}` returned a non-numeric component \
                         at embedding {index} position {position}"
                    ),
                    false,
                )
            })?;
            vector.push(number as f32);
        }
        if vector.is_empty() {
            return Err(CoreError::provider(
                provider.to_string(),
                format!(
                    "provider=`{provider}` model=`{model}` returned an empty vector for embedding {index}"
                ),
                false,
            ));
        }
        vectors.push(vector);
    }

    let mut response = EmbeddingResponse::new(provider.clone(), model)
        .with_vectors(vectors)
        .with_transport(transport);
    if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
        response = response.with_usage(parse_usage(usage));
    }
    Ok(response)
}

/// Injects credentials and custom headers.
///
/// The plaintext leaves [`crate::secret::SecretString`] only inside this function,
/// is moved straight into a `HeaderValue`, and is never bound to a variable that
/// could be logged.
fn authorize(mut builder: RequestBuilder, provider: &ProviderConfig) -> CoreResult<RequestBuilder> {
    if let Some(api_key) = &provider.api_key {
        let value = HeaderValue::from_str(&format!("Bearer {}", api_key.expose_secret())).map_err(
            |_| {
                CoreError::config_field(
                    format!("providers.{}.api_key", provider.id),
                    "contains characters that are not allowed in an HTTP header",
                )
            },
        )?;
        builder = builder.header(reqwest::header::AUTHORIZATION, value);
    }
    for (name, value) in &provider.extra_headers {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            CoreError::config_field(
                format!("providers.{}.extra_headers", provider.id),
                format!("`{name}` is not a valid header name"),
            )
        })?;
        let header_value = HeaderValue::from_str(value.expose_secret()).map_err(|_| {
            CoreError::config_field(
                format!("providers.{}.extra_headers", provider.id),
                format!("`{name}` contains characters that are not allowed in an HTTP header"),
            )
        })?;
        builder = builder.header(header_name, header_value);
    }
    Ok(builder)
}

fn message_payload(message: &Message) -> Value {
    let role = message.role().as_str();

    if message.role() == Role::Tool {
        return json!({
            "role": "tool",
            "tool_call_id": message.tool_call_id().unwrap_or_default(),
            "content": message.text_content(),
        });
    }

    if message.role() == Role::Assistant && !message.tool_calls().is_empty() {
        let calls: Vec<Value> = message
            .tool_calls()
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {"name": call.name, "arguments": call.arguments},
                })
            })
            .collect();
        return json!({
            "role": "assistant",
            "content": message.text_content(),
            "tool_calls": calls,
        });
    }

    if message.parts().len() == 1 {
        if let Some(text) = message.parts().first().and_then(ContentPart::as_text) {
            return json!({"role": role, "content": text});
        }
    }
    let parts: Vec<Value> = message
        .parts()
        .iter()
        .map(|part| match part {
            ContentPart::Text(text) => json!({"type": "text", "text": text}),
            ContentPart::Image { url } => json!({"type": "image_url", "image_url": {"url": url}}),
        })
        .collect();
    json!({"role": role, "content": parts})
}

fn parse_usage(value: &Value) -> UsageReport {
    UsageReport {
        prompt_tokens: u32_at(value, "prompt_tokens"),
        completion_tokens: u32_at(value, "completion_tokens"),
        total_tokens: u32_at(value, "total_tokens"),
        reasoning_tokens: value
            .get("completion_tokens_details")
            .and_then(|details| u32_at(details, "reasoning_tokens")),
        cached_tokens: value
            .get("prompt_tokens_details")
            .and_then(|details| u32_at(details, "cached_tokens")),
        raw: Some(value.clone()),
    }
}

fn u32_at(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|raw| u32::try_from(raw).ok())
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn map_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("stop") => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        Some("tool_calls") | Some("function_call") => FinishReason::ToolCalls,
        Some("content_filter") => FinishReason::ContentFilter,
        Some(_) => FinishReason::Unknown,
        None => FinishReason::Incomplete,
    }
}

fn extract_content(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| str_at(part, "text"))
            .collect::<String>(),
        _ => String::new(),
    }
}

fn extract_tool_calls(message: &Value) -> Vec<ToolCall> {
    message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .map(|call| {
                    let function = call.get("function");
                    ToolCall {
                        id: str_at(call, "id").unwrap_or_default().to_owned(),
                        name: function
                            .and_then(|function| str_at(function, "name"))
                            .unwrap_or_default()
                            .to_owned(),
                        arguments: function
                            .and_then(|function| str_at(function, "arguments"))
                            .unwrap_or_default()
                            .to_owned(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_response(
    provider: &ProviderId,
    model: &str,
    value: &Value,
    transport: TransportStats,
) -> CoreResult<AiResponse> {
    let choice = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .ok_or_else(|| {
            CoreError::provider(
                provider.to_string(),
                format!("provider=`{provider}` model=`{model}` returned no choices"),
                false,
            )
        })?;

    let message = choice.get("message").cloned().unwrap_or(Value::Null);
    let content = extract_content(&message);
    let reasoning = str_at(&message, "reasoning_content")
        .or_else(|| str_at(&message, "reasoning"))
        .map(str::to_owned);
    let tool_calls = extract_tool_calls(&message);
    let finish_reason = map_finish_reason(str_at(choice, "finish_reason"));

    let mut response = AiResponse::new(provider.clone(), model)
        .with_content(content)
        .with_tool_calls(tool_calls)
        .with_finish_reason(finish_reason)
        .with_transport(transport);
    if let Some(reasoning) = reasoning {
        response = response.with_reasoning(reasoning);
    }
    if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
        response = response.with_usage(parse_usage(usage));
    }
    Ok(response)
}

/// Events decoded from one `data:` frame, plus the finish reason it announced.
type ChunkEvents = (Vec<StreamEvent>, Option<FinishReason>);

fn parse_stream_chunk(data: &str, provider: &ProviderId, model: &str) -> CoreResult<ChunkEvents> {
    let value: Value = serde_json::from_str(data).map_err(|error| {
        http::decode_error(provider, model, "OpenAI-compatible stream frame", error)
    })?;

    if let Some(error) = value.get("error") {
        let message = str_at(error, "message").unwrap_or("unknown stream error");
        let retryable = matches!(
            str_at(error, "code").unwrap_or_default(),
            "rate_limit_exceeded" | "server_error"
        );
        return Err(CoreError::provider(
            provider.to_string(),
            format!(
                "provider=`{provider}` model=`{model}` reported a stream error: {}",
                http::body_snippet(message)
            ),
            retryable,
        ));
    }

    let mut events = Vec::new();
    if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
        events.push(StreamEvent::Usage(parse_usage(usage)));
    }

    let mut finish_reason = None;
    if let Some(choice) = value.get("choices").and_then(|choices| choices.get(0)) {
        if let Some(delta) = choice.get("delta") {
            if let Some(text) = str_at(delta, "content") {
                if !text.is_empty() {
                    events.push(StreamEvent::TextDelta {
                        text: text.to_owned(),
                    });
                }
            }
            if let Some(text) =
                str_at(delta, "reasoning_content").or_else(|| str_at(delta, "reasoning"))
            {
                if !text.is_empty() {
                    events.push(StreamEvent::ReasoningDelta {
                        text: text.to_owned(),
                    });
                }
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for (position, call) in calls.iter().enumerate() {
                    let index = call
                        .get("index")
                        .and_then(Value::as_u64)
                        .and_then(|raw| u16::try_from(raw).ok())
                        .unwrap_or(u16::try_from(position).unwrap_or(0));
                    let function = call.get("function");
                    events.push(StreamEvent::ToolCallDelta {
                        index,
                        id: str_at(call, "id").map(str::to_owned),
                        name: function
                            .and_then(|function| str_at(function, "name"))
                            .map(str::to_owned),
                        arguments: function
                            .and_then(|function| str_at(function, "arguments"))
                            .unwrap_or_default()
                            .to_owned(),
                    });
                }
            }
        }
        if let Some(reason) = str_at(choice, "finish_reason") {
            finish_reason = Some(map_finish_reason(Some(reason)));
        }
    }
    Ok((events, finish_reason))
}

impl ProviderAdapter for OpenAiCompatibleAdapter {
    fn protocol(&self) -> ProviderProtocol {
        ProviderProtocol::OpenAiCompatible
    }

    fn label(&self) -> &str {
        "openai-compatible"
    }

    fn embed<'a>(
        &'a self,
        call: EmbeddingCall<'a>,
    ) -> BoxFuture<'a, CoreResult<EmbeddingResponse>> {
        Box::pin(async move {
            let payload = serde_json::to_vec(&encode_embedding_body(&call)).map_err(|error| {
                CoreError::serialization_with_source(
                    "failed to encode the OpenAI-compatible embedding body",
                    error,
                )
            })?;
            let request_bytes = u64::try_from(payload.len()).unwrap_or(u64::MAX);
            let builder = self
                .client
                .post(call.provider.endpoint(EMBEDDINGS_PATH))
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "application/json")
                .body(payload);
            let builder = authorize(builder, call.provider)?;

            let provider = &call.provider.id;
            let model = call.model.id.as_str();
            let response = builder
                .send()
                .await
                .map_err(|error| http::network_error(provider, model, error))?;
            let status = response.status();
            let bytes = response
                .bytes()
                .await
                .map_err(|error| http::network_error(provider, model, error))?;
            let response_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            let body = String::from_utf8_lossy(&bytes);

            if !status.is_success() {
                return Err(http::status_error(provider, model, status, body.as_ref()));
            }
            let value: Value = serde_json::from_str(body.as_ref()).map_err(|error| {
                http::decode_error(provider, model, "OpenAI-compatible embedding body", error)
            })?;
            parse_embedding_response(
                provider,
                model,
                &value,
                TransportStats {
                    request_bytes,
                    response_bytes,
                },
            )
        })
    }

    fn complete<'a>(&'a self, call: AdapterCall<'a>) -> BoxFuture<'a, CoreResult<AiResponse>> {
        Box::pin(async move {
            let (builder, request_bytes) = self.build_request(&call, false)?;
            let provider = &call.provider.id;
            let model = call.model.id.as_str();

            let response = builder
                .send()
                .await
                .map_err(|error| http::network_error(provider, model, error))?;
            let status = response.status();
            let bytes = response
                .bytes()
                .await
                .map_err(|error| http::network_error(provider, model, error))?;
            let response_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            let body = String::from_utf8_lossy(&bytes);

            if !status.is_success() {
                return Err(http::status_error(provider, model, status, body.as_ref()));
            }
            let value: Value = serde_json::from_str(body.as_ref()).map_err(|error| {
                http::decode_error(provider, model, "OpenAI-compatible response body", error)
            })?;
            parse_response(
                provider,
                model,
                &value,
                TransportStats {
                    request_bytes,
                    response_bytes,
                },
            )
        })
    }

    fn stream<'a>(&'a self, call: AdapterCall<'a>) -> BoxFuture<'a, CoreResult<AiStream>> {
        Box::pin(async move {
            let (builder, request_bytes) = self.build_request(&call, true)?;
            let provider = call.provider.id.clone();
            let model = call.model.id.clone();
            let idle = call.streaming_idle;
            let cancel = call.cancel.clone();

            let response = builder
                .send()
                .await
                .map_err(|error| http::network_error(&provider, &model, error))?;
            let status = response.status();
            if !status.is_success() {
                let bytes = response
                    .bytes()
                    .await
                    .map_err(|error| http::network_error(&provider, &model, error))?;
                return Err(http::status_error(
                    &provider,
                    &model,
                    status,
                    String::from_utf8_lossy(&bytes).as_ref(),
                ));
            }

            let (sender, receiver) = mpsc::channel(STREAM_CHANNEL_CAPACITY);
            let stream = AiStream::new(provider.clone(), model.clone(), receiver);

            tokio::spawn(async move {
                if sender
                    .send(Ok(StreamEvent::Started {
                        provider_id: provider.clone(),
                        model_id: model.clone(),
                    }))
                    .await
                    .is_err()
                {
                    return;
                }

                let mut response = response;
                let mut decoder = SseDecoder::new();
                let mut response_bytes = 0_u64;
                let mut finish_reason: Option<FinishReason> = None;
                let mut usage: Option<UsageReport> = None;
                let mut saw_done = false;

                'reading: loop {
                    let chunk = tokio::select! {
                        biased;
                        () = cancel.cancelled() => {
                            let _ = sender.send(Err(CoreError::cancelled("provider.stream"))).await;
                            return;
                        }
                        result = tokio::time::timeout(idle, response.chunk()) => match result {
                            Err(_) => {
                                let _ = sender
                                    .send(Err(CoreError::timeout("provider.stream_idle", idle)))
                                    .await;
                                return;
                            }
                            Ok(Err(error)) => {
                                let _ = sender
                                    .send(Err(http::network_error(&provider, &model, error)))
                                    .await;
                                return;
                            }
                            Ok(Ok(None)) => break 'reading,
                            Ok(Ok(Some(chunk))) => chunk,
                        },
                    };

                    response_bytes += u64::try_from(chunk.len()).unwrap_or(0);
                    decoder.push(&chunk);
                    while let Some(data) = decoder.next_event() {
                        if data.trim() == "[DONE]" {
                            saw_done = true;
                            break 'reading;
                        }
                        match parse_stream_chunk(&data, &provider, &model) {
                            Ok((events, reason)) => {
                                if let Some(reason) = reason {
                                    finish_reason = Some(reason);
                                }
                                for event in events {
                                    if let StreamEvent::Usage(report) = &event {
                                        usage = Some(report.clone());
                                    }
                                    if sender.send(Ok(event)).await.is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(error) => {
                                let _ = sender.send(Err(error)).await;
                                return;
                            }
                        }
                    }
                }

                let finish_reason = match (finish_reason, saw_done) {
                    (Some(reason), _) => reason,
                    (None, true) => FinishReason::Stop,
                    (None, false) => FinishReason::Incomplete,
                };
                let _ = sender
                    .send(Ok(StreamEvent::Finished {
                        finish_reason,
                        usage,
                        transport: TransportStats {
                            request_bytes,
                            response_bytes,
                        },
                    }))
                    .await;
            });

            Ok(stream)
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::provider::config::{ModelConfig, ProviderConfig};

    fn provider() -> ProviderConfig {
        ProviderConfig::new(
            ProviderId::new("openai").unwrap(),
            ProviderProtocol::OpenAiCompatible,
        )
        .with_api_key("sk-test")
        .with_base_url("https://api.example.com/v1/")
    }

    fn model() -> ModelConfig {
        ModelConfig::new("gpt-4o-mini")
    }

    fn body_for(request: &crate::provider::types::AiRequest, stream: bool) -> Value {
        let provider = provider();
        let model = model();
        let trace = crate::observability::TraceContext::generate();
        let cancel = crate::async_utils::CancelToken::new();
        let call = AdapterCall {
            request,
            provider: &provider,
            model: &model,
            trace: &trace,
            cancel: &cancel,
            streaming_idle: Duration::from_secs(1),
        };
        OpenAiCompatibleAdapter::encode_body(&call, stream)
    }

    #[test]
    fn encodes_embedding_batches_without_messages() {
        let provider = provider();
        let model = model();
        let trace = crate::observability::TraceContext::generate();
        let cancel = crate::async_utils::CancelToken::new();
        let request = crate::provider::types::EmbeddingRequest::new(["apple", "pear"])
            .unwrap()
            .with_dimensions(64);
        let call = EmbeddingCall {
            request: &request,
            provider: &provider,
            model: &model,
            trace: &trace,
            cancel: &cancel,
        };
        let body = encode_embedding_body(&call);
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["input"][0], "apple");
        assert_eq!(body["input"][1], "pear");
        assert_eq!(body["dimensions"], 64);
        assert_eq!(body["encoding_format"], "float");
        assert!(body.get("messages").is_none());
    }

    #[test]
    fn embedding_vectors_keep_input_order_and_usage() {
        let value = json!({
            "data": [
                {"embedding": [0.5, -0.25]},
                {"embedding": [1.0, 0.0]}
            ],
            "usage": {"prompt_tokens": 2, "total_tokens": 2}
        });
        let provider = ProviderId::new("openai").unwrap();
        let response = parse_embedding_response(
            &provider,
            "text-embedding-3-small",
            &value,
            TransportStats::default(),
        )
        .unwrap();

        assert_eq!(response.len(), 2);
        assert_eq!(response.dimensions(), Some(2));
        assert_eq!(response.vectors()[0], vec![0.5, -0.25]);
        assert_eq!(response.vectors()[1], vec![1.0, 0.0]);
        assert_eq!(
            response.usage().and_then(|usage| usage.total_tokens),
            Some(2)
        );
    }

    #[test]
    fn malformed_embedding_payloads_are_rejected() {
        let provider = ProviderId::new("openai").unwrap();
        for value in [
            json!({}),
            json!({"data": [{}]}),
            json!({"data": [{"embedding": []}]}),
            json!({"data": [{"embedding": ["x"]}]}),
        ] {
            let error = parse_embedding_response(&provider, "m", &value, TransportStats::default())
                .expect_err("must be rejected");
            assert!(matches!(error, CoreError::Provider { .. }));
        }
    }

    #[test]
    fn encodes_messages_parameters_and_stream_flag() {
        let request = crate::provider::types::AiRequest::user_prompt("hello")
            .with_temperature(0.5)
            .with_max_output_tokens(64)
            .with_stop(["END"]);
        let body = body_for(&request, true);
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hello");
        assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["stop"][0], "END");
        assert!(body["temperature"].is_number());
    }

    #[test]
    fn encodes_multimodal_content_and_schema() {
        let request =
            crate::provider::types::AiRequest::new([crate::provider::types::Message::user(
                "what is this?",
            )
            .with_image("https://example.com/a.png")])
            .with_response_format(ResponseFormat::JsonSchema {
                name: "answer".to_owned(),
                schema: json!({"type": "object"}),
            });
        let body = body_for(&request, false);
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["response_format"]["json_schema"]["name"], "answer");
    }

    #[test]
    fn parses_text_reasoning_and_usage() {
        let value = json!({
            "choices": [{
                "message": {
                    "content": "answer",
                    "reasoning_content": "thinking",
                    "tool_calls": [{"id": "c1", "function": {"name": "lookup", "arguments": "{\"q\":1}"}}]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7}
        });
        let provider = ProviderId::new("openai").unwrap();
        let response =
            parse_response(&provider, "gpt-4o-mini", &value, TransportStats::default()).unwrap();
        assert_eq!(response.content(), "answer");
        assert_eq!(response.reasoning(), Some("thinking"));
        assert_eq!(response.finish_reason(), FinishReason::ToolCalls);
        assert_eq!(response.tool_calls().len(), 1);
        assert_eq!(
            response.usage().and_then(|usage| usage.total_tokens),
            Some(7)
        );
    }

    #[test]
    fn stream_frames_produce_ordered_events() {
        let provider = ProviderId::new("openai").unwrap();
        let (events, reason) = parse_stream_chunk(
            r#"{"choices":[{"delta":{"content":"he"}}]}"#,
            &provider,
            "m",
        )
        .unwrap();
        assert_eq!(reason, None);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].text(), Some("he"));

        let (events, reason) = parse_stream_chunk(
            r#"{"choices":[{"delta":{},"finish_reason":"length"}],"usage":{"total_tokens":9}}"#,
            &provider,
            "m",
        )
        .unwrap();
        assert_eq!(reason, Some(FinishReason::Length));
        assert!(matches!(events[0], StreamEvent::Usage(_)));
    }

    #[test]
    fn stream_error_frames_become_provider_errors() {
        let provider = ProviderId::new("openai").unwrap();
        let error = parse_stream_chunk(
            r#"{"error":{"message":"rate limit reached","code":"rate_limit_exceeded"}}"#,
            &provider,
            "m",
        )
        .unwrap_err();
        assert!(error.is_retryable());
        assert!(matches!(error, CoreError::Provider { .. }));
    }
}
