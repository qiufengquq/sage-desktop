//! Anthropic Messages adapter.
//!
//! Endpoints used:
//!
//! * `POST {base_url}/messages`
//! * authentication: `x-api-key: <api_key>` plus `anthropic-version`
//! * streaming: `text/event-stream` with `message_start`, `content_block_delta`,
//!   `message_delta` and `message_stop` frames
//!
//! Protocol differences handled here (and nowhere else):
//!
//! * the system prompt is a top-level `system` field, not a message;
//! * `max_tokens` is mandatory;
//! * stop sequences are called `stop_sequences`;
//! * tool schemas use `input_schema`;
//! * usage is reported as `input_tokens` / `output_tokens`;
//! * images use `{"type":"image","source":{"type":"url","url":...}}`.

use reqwest::header::{HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::{Client, RequestBuilder};
use serde_json::{json, Map, Value};
use tokio::sync::mpsc;

use crate::error::{CoreError, CoreResult};
use crate::provider::adapter::{AdapterCall, BoxFuture, ProviderAdapter};
use crate::provider::config::{ProviderConfig, ProviderId, ProviderProtocol};
use crate::provider::http::{self, SseDecoder};
use crate::provider::response::{
    AiResponse, AiStream, FinishReason, StreamEvent, ToolCall, TransportStats, UsageReport,
};
use crate::provider::types::{ContentPart, Message, ResponseFormat, Role};

/// Path appended to `base_url` for the messages endpoint.
const MESSAGES_PATH: &str = "messages";
/// Required `anthropic-version` header value.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Fallback `max_tokens` when the request does not specify one.
const DEFAULT_MAX_TOKENS: u32 = 1_024;
/// Capacity of the channel between the HTTP reader task and the consumer.
const STREAM_CHANNEL_CAPACITY: usize = 32;

/// Adapter for the Anthropic Messages wire format.
#[derive(Debug, Clone)]
pub struct AnthropicAdapter {
    client: Client,
}

impl AnthropicAdapter {
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

    fn encode_body(call: &AdapterCall<'_>, stream: bool) -> CoreResult<Value> {
        let mut system: Vec<&str> = Vec::new();
        let mut messages: Vec<Value> = Vec::new();
        // Consecutive tool results belong to one `user` turn, which is how the
        // Messages API models them.
        let mut open_results: Option<usize> = None;
        for message in call.request.messages() {
            match message.role() {
                Role::System => {
                    system.push(match message.parts().first() {
                        Some(ContentPart::Text(text)) => text.as_str(),
                        _ => "",
                    });
                    open_results = None;
                }
                Role::Tool => {
                    let block = json!({
                        "type": "tool_result",
                        "tool_use_id": message.tool_call_id().unwrap_or_default(),
                        "content": message.text_content(),
                    });
                    match open_results {
                        Some(index) => {
                            if let Some(Value::Array(content)) = messages
                                .get_mut(index)
                                .and_then(|entry| entry.get_mut("content"))
                            {
                                content.push(block);
                            }
                        }
                        None => {
                            messages.push(json!({"role": "user", "content": [block]}));
                            open_results = Some(messages.len() - 1);
                        }
                    }
                }
                Role::User | Role::Assistant => {
                    open_results = None;
                    messages.push(message_payload(message));
                }
            }
        }

        let mut body = Map::new();
        body.insert("model".to_owned(), json!(call.model.id));
        body.insert("messages".to_owned(), Value::Array(messages));
        body.insert(
            "max_tokens".to_owned(),
            json!(call
                .request
                .max_output_tokens()
                .unwrap_or(DEFAULT_MAX_TOKENS)),
        );
        body.insert("stream".to_owned(), json!(stream));
        if !system.is_empty() {
            body.insert("system".to_owned(), json!(system.join("\n\n")));
        }
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
        if !call.request.stop().is_empty() {
            body.insert("stop_sequences".to_owned(), json!(call.request.stop()));
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
                                "name": tool.name(),
                                "description": tool.description(),
                                "input_schema": tool.parameters(),
                            })
                        })
                        .collect(),
                ),
            );
        }
        match call.request.response_format() {
            ResponseFormat::Text => {}
            ResponseFormat::JsonObject | ResponseFormat::JsonSchema { .. } => {
                // Anthropic has no `response_format`; the documented approach is a
                // system instruction. It is appended so the caller's prompt stays
                // untouched, and the capability check ensures the model declares
                // structured output support before we get here.
                let instruction = "Respond with a single valid JSON object and nothing else.";
                let combined = match body.get("system").and_then(Value::as_str) {
                    Some(existing) => format!("{existing}\n\n{instruction}"),
                    None => instruction.to_owned(),
                };
                body.insert("system".to_owned(), json!(combined));
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
        Ok(Value::Object(body))
    }

    fn build_request(
        &self,
        call: &AdapterCall<'_>,
        stream: bool,
    ) -> CoreResult<(RequestBuilder, u64)> {
        let body = Self::encode_body(call, stream)?;
        let payload = serde_json::to_vec(&body).map_err(|error| {
            CoreError::serialization_with_source(
                "failed to encode the Anthropic request body",
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
            .post(call.provider.endpoint(MESSAGES_PATH))
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, accept)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .body(payload);

        builder = authorize(builder, call.provider)?;
        Ok((builder, request_bytes))
    }
}

fn authorize(mut builder: RequestBuilder, provider: &ProviderConfig) -> CoreResult<RequestBuilder> {
    if let Some(api_key) = &provider.api_key {
        let value = HeaderValue::from_str(api_key.expose_secret()).map_err(|_| {
            CoreError::config_field(
                format!("providers.{}.api_key", provider.id),
                "contains characters that are not allowed in an HTTP header",
            )
        })?;
        builder = builder.header("x-api-key", value);
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
    let role = match message.role() {
        Role::Assistant => "assistant",
        _ => "user",
    };
    if message.tool_calls().is_empty() && message.parts().len() == 1 {
        if let Some(text) = message.parts().first().and_then(ContentPart::as_text) {
            return json!({"role": role, "content": text});
        }
    }
    // An assistant turn that only calls tools carries a placeholder empty text
    // part; Anthropic rejects empty text blocks, and the tool_use blocks must be
    // first in the turn.
    let skip_empty_text = !message.tool_calls().is_empty();
    let mut parts: Vec<Value> = message
        .parts()
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text(text) if skip_empty_text && text.is_empty() => None,
            ContentPart::Text(text) => Some(json!({"type": "text", "text": text})),
            ContentPart::Image { url } => Some(json!({
                "type": "image",
                "source": {"type": "url", "url": url}
            })),
        })
        .collect();
    for call in message.tool_calls() {
        // `input` must be an object; an unparsable argument string becomes `{}`
        // rather than an invalid payload the provider would reject wholesale.
        let input = serde_json::from_str::<Value>(&call.arguments).unwrap_or_else(|_| json!({}));
        parts.push(json!({
            "type": "tool_use",
            "id": call.id,
            "name": call.name,
            "input": input,
        }));
    }
    json!({"role": role, "content": parts})
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

fn parse_usage(value: &Value) -> UsageReport {
    let prompt = u32_at(value, "input_tokens");
    let completion = u32_at(value, "output_tokens");
    UsageReport {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: match (prompt, completion) {
            (Some(prompt), Some(completion)) => prompt.checked_add(completion),
            _ => None,
        },
        reasoning_tokens: None,
        cached_tokens: u32_at(value, "cache_read_input_tokens"),
        raw: Some(value.clone()),
    }
}

fn map_stop_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("end_turn") | Some("stop_sequence") => FinishReason::Stop,
        Some("max_tokens") => FinishReason::Length,
        Some("tool_use") => FinishReason::ToolCalls,
        Some("refusal") => FinishReason::ContentFilter,
        Some(_) => FinishReason::Unknown,
        None => FinishReason::Incomplete,
    }
}

fn parse_response(
    provider: &ProviderId,
    model: &str,
    value: &Value,
    transport: TransportStats,
) -> CoreResult<AiResponse> {
    let blocks = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            CoreError::provider(
                provider.to_string(),
                format!("provider=`{provider}` model=`{model}` returned no content blocks"),
                false,
            )
        })?;

    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    for block in blocks {
        match str_at(block, "type") {
            Some("text") => content.push_str(str_at(block, "text").unwrap_or_default()),
            Some("thinking") => reasoning.push_str(str_at(block, "thinking").unwrap_or_default()),
            Some("tool_use") => {
                let arguments = block
                    .get("input")
                    .and_then(|input| serde_json::to_string(input).ok())
                    .unwrap_or_else(|| "{}".to_owned());
                tool_calls.push(ToolCall {
                    id: str_at(block, "id").unwrap_or_default().to_owned(),
                    name: str_at(block, "name").unwrap_or_default().to_owned(),
                    arguments,
                });
            }
            _ => {}
        }
    }

    let mut response = AiResponse::new(provider.clone(), model)
        .with_content(content)
        .with_tool_calls(tool_calls)
        .with_finish_reason(map_stop_reason(str_at(value, "stop_reason")))
        .with_transport(transport);
    if !reasoning.is_empty() {
        response = response.with_reasoning(reasoning);
    }
    if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
        response = response.with_usage(parse_usage(usage));
    }
    Ok(response)
}

/// Decoded effect of one Anthropic stream frame.
#[derive(Debug)]
struct FrameEffect {
    events: Vec<StreamEvent>,
    finish_reason: Option<FinishReason>,
    usage: Option<UsageReport>,
    stop: bool,
}

fn parse_stream_frame(data: &str, provider: &ProviderId, model: &str) -> CoreResult<FrameEffect> {
    let value: Value = serde_json::from_str(data)
        .map_err(|error| http::decode_error(provider, model, "Anthropic stream frame", error))?;

    let mut effect = FrameEffect {
        events: Vec::new(),
        finish_reason: None,
        usage: None,
        stop: false,
    };

    match str_at(&value, "type") {
        Some("error") => {
            let error = value.get("error").cloned().unwrap_or(Value::Null);
            let kind = str_at(&error, "type").unwrap_or("unknown_error");
            let message = str_at(&error, "message").unwrap_or("unknown stream error");
            let retryable = matches!(kind, "overloaded_error" | "rate_limit_error" | "api_error");
            return Err(CoreError::provider(
                provider.to_string(),
                format!(
                    "provider=`{provider}` model=`{model}` reported `{kind}`: {}",
                    http::body_snippet(message)
                ),
                retryable,
            ));
        }
        Some("message_start") => {
            if let Some(usage) = value
                .get("message")
                .and_then(|message| message.get("usage"))
            {
                effect.usage = Some(parse_usage(usage));
            }
        }
        Some("content_block_delta") => {
            let index = value
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|raw| u16::try_from(raw).ok())
                .unwrap_or(0);
            if let Some(delta) = value.get("delta") {
                match str_at(delta, "type") {
                    Some("text_delta") => {
                        if let Some(text) = str_at(delta, "text") {
                            effect.events.push(StreamEvent::TextDelta {
                                text: text.to_owned(),
                            });
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(text) = str_at(delta, "thinking") {
                            effect.events.push(StreamEvent::ReasoningDelta {
                                text: text.to_owned(),
                            });
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(partial) = str_at(delta, "partial_json") {
                            effect.events.push(StreamEvent::ToolCallDelta {
                                index,
                                id: None,
                                name: None,
                                arguments: partial.to_owned(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        Some("content_block_start") => {
            let index = value
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|raw| u16::try_from(raw).ok())
                .unwrap_or(0);
            if let Some(block) = value.get("content_block") {
                if str_at(block, "type") == Some("tool_use") {
                    effect.events.push(StreamEvent::ToolCallDelta {
                        index,
                        id: str_at(block, "id").map(str::to_owned),
                        name: str_at(block, "name").map(str::to_owned),
                        arguments: String::new(),
                    });
                }
            }
        }
        Some("message_delta") => {
            effect.finish_reason = Some(map_stop_reason(
                value
                    .get("delta")
                    .and_then(|delta| str_at(delta, "stop_reason")),
            ));
            if let Some(usage) = value.get("usage") {
                effect.usage = Some(parse_usage(usage));
            }
        }
        Some("message_stop") => {
            effect.stop = true;
        }
        _ => {}
    }
    Ok(effect)
}

fn merge_usage(target: &mut UsageReport, update: UsageReport) {
    if update.prompt_tokens.is_some() {
        target.prompt_tokens = update.prompt_tokens;
    }
    if update.completion_tokens.is_some() {
        target.completion_tokens = update.completion_tokens;
    }
    if update.cached_tokens.is_some() {
        target.cached_tokens = update.cached_tokens;
    }
    if update.raw.is_some() {
        target.raw = update.raw.clone();
    }
    target.total_tokens = match (target.prompt_tokens, target.completion_tokens) {
        (Some(prompt), Some(completion)) => prompt.checked_add(completion),
        _ => target.total_tokens,
    };
}

impl ProviderAdapter for AnthropicAdapter {
    fn protocol(&self) -> ProviderProtocol {
        ProviderProtocol::Anthropic
    }

    fn label(&self) -> &str {
        "anthropic"
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
                http::decode_error(provider, model, "Anthropic response body", error)
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
                let mut usage = UsageReport::default();
                let mut stopped = false;

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
                        match parse_stream_frame(&data, &provider, &model) {
                            Ok(effect) => {
                                if let Some(report) = effect.usage {
                                    merge_usage(&mut usage, report);
                                }
                                if let Some(reason) = effect.finish_reason {
                                    finish_reason = Some(reason);
                                }
                                for event in effect.events {
                                    if sender.send(Ok(event)).await.is_err() {
                                        return;
                                    }
                                }
                                if effect.stop {
                                    stopped = true;
                                    break 'reading;
                                }
                            }
                            Err(error) => {
                                let _ = sender.send(Err(error)).await;
                                return;
                            }
                        }
                    }
                }

                let finish_reason = match (finish_reason, stopped) {
                    (Some(reason), _) => reason,
                    (None, true) => FinishReason::Stop,
                    (None, false) => FinishReason::Incomplete,
                };
                let usage = if usage.is_empty() { None } else { Some(usage) };
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

    use crate::provider::config::ModelConfig;
    use crate::provider::types::AiRequest;

    fn provider() -> ProviderConfig {
        ProviderConfig::new(
            ProviderId::new("anthropic").unwrap(),
            ProviderProtocol::Anthropic,
        )
        .with_api_key("sk-ant-test")
    }

    fn body_for(request: &AiRequest, stream: bool) -> Value {
        let provider = provider();
        let model = ModelConfig::new("claude-3-5-sonnet-latest");
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
        AnthropicAdapter::encode_body(&call, stream).unwrap()
    }

    #[test]
    fn system_prompt_is_lifted_out_of_messages() {
        let request = AiRequest::new([Message::system("be brief"), Message::user("hello")]);
        let body = body_for(&request, false);
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["messages"].as_array().map(Vec::len), Some(1));
        assert_eq!(body["messages"][0]["role"], "user");
        assert!(body["max_tokens"].is_number());
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn tool_results_are_encoded_as_user_blocks() {
        let request = AiRequest::new([
            Message::user("look it up"),
            Message::tool_result("call_1", "{\"word\":\"apple\"}"),
            Message::tool_result("call_2", "42"),
        ]);
        let body = body_for(&request, false);
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][1]["content"][0]["tool_use_id"], "call_1");
        // Two consecutive results share one user turn.
        assert_eq!(body["messages"][1]["content"][1]["tool_use_id"], "call_2");
        assert_eq!(body["messages"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn assistant_tool_calls_become_tool_use_blocks() {
        let request = AiRequest::new([
            Message::user("look it up"),
            Message::assistant_tool_calls(
                "",
                vec![crate::provider::response::ToolCall {
                    id: "call_1".to_owned(),
                    name: "lookup_word".to_owned(),
                    arguments: "{\"word\":\"apple\"}".to_owned(),
                }],
            ),
        ]);
        let body = body_for(&request, false);
        let block = &body["messages"][1]["content"][0];
        assert_eq!(block["type"], "tool_use");
        assert_eq!(block["id"], "call_1");
        assert_eq!(block["input"]["word"], "apple");
    }

    #[test]
    fn structured_output_appends_a_system_instruction() {
        let request = AiRequest::user_prompt("hi").with_response_format(ResponseFormat::JsonObject);
        let body = body_for(&request, false);
        assert!(body["system"]
            .as_str()
            .map(|system| system.contains("JSON"))
            .unwrap_or(false));
    }

    #[test]
    fn parses_text_thinking_and_tool_use() {
        let value = json!({
            "content": [
                {"type": "thinking", "thinking": "hmm"},
                {"type": "text", "text": "answer"},
                {"type": "tool_use", "id": "t1", "name": "lookup", "input": {"q": "x"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 5, "output_tokens": 6}
        });
        let provider = ProviderId::new("anthropic").unwrap();
        let response =
            parse_response(&provider, "claude", &value, TransportStats::default()).unwrap();
        assert_eq!(response.content(), "answer");
        assert_eq!(response.reasoning(), Some("hmm"));
        assert_eq!(response.finish_reason(), FinishReason::ToolCalls);
        assert_eq!(response.tool_calls()[0].arguments, "{\"q\":\"x\"}");
        assert_eq!(
            response.usage().and_then(|usage| usage.total_tokens),
            Some(11)
        );
    }

    #[test]
    fn stream_frames_are_decoded() {
        let provider = ProviderId::new("anthropic").unwrap();

        let effect = parse_stream_frame(
            r#"{"type":"message_start","message":{"usage":{"input_tokens":7}}}"#,
            &provider,
            "claude",
        )
        .unwrap();
        assert_eq!(effect.usage.and_then(|usage| usage.prompt_tokens), Some(7));

        let effect = parse_stream_frame(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
            &provider,
            "claude",
        )
        .unwrap();
        assert_eq!(effect.events[0].text(), Some("hi"));

        let effect = parse_stream_frame(
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}"#,
            &provider,
            "claude",
        )
        .unwrap();
        assert_eq!(effect.finish_reason, Some(FinishReason::Stop));

        let effect = parse_stream_frame(r#"{"type":"message_stop"}"#, &provider, "claude").unwrap();
        assert!(effect.stop);
    }

    #[test]
    fn stream_error_frames_map_to_provider_errors() {
        let provider = ProviderId::new("anthropic").unwrap();
        let error = parse_stream_frame(
            r#"{"type":"error","error":{"type":"overloaded_error","message":"try later"}}"#,
            &provider,
            "claude",
        )
        .unwrap_err();
        assert!(error.is_retryable());
    }

    #[test]
    fn usage_merge_accumulates_prompt_and_completion() {
        let mut usage = UsageReport::default();
        merge_usage(
            &mut usage,
            UsageReport {
                prompt_tokens: Some(3),
                ..UsageReport::default()
            },
        );
        merge_usage(
            &mut usage,
            UsageReport {
                completion_tokens: Some(4),
                ..UsageReport::default()
            },
        );
        assert_eq!(usage.total_tokens, Some(7));
        assert!(!usage.is_empty());
    }
}
