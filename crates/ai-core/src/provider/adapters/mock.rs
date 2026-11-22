//! In-process scripted adapter for tests, local development and demos.
//!
//! `MockAdapter` implements the same [`ProviderAdapter`] contract as the real
//! adapters, so registry, router, retry, fallback, capability validation,
//! observability and streaming code paths are exercised **without any network
//! access**. Scripted steps are consumed in order; when the script runs out the
//! adapter keeps answering with a canned success, which keeps long test loops
//! simple.
//!
//! Every call is recorded ([`MockAdapter::calls`]) with its provider, model,
//! trace/request ids and payload shape — enough to assert routing, retry and
//! hot-reload behaviour without inspecting private state.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::error::CoreError;
use crate::ids::{RequestId, TraceId};
use crate::provider::adapter::{AdapterCall, BoxFuture, EmbeddingCall, ProviderAdapter};
use crate::provider::config::{ProviderId, ProviderProtocol};
use crate::provider::response::{
    AiResponse, AiStream, EmbeddingResponse, FinishReason, StreamEvent, ToolCall, TransportStats,
    UsageReport,
};
use crate::provider::types::ContentPart;
use crate::CoreResult;

/// Canned content returned once the script is exhausted.
const DEFAULT_CONTENT: &str = "mock response";
/// Capacity of the mock stream channel.
const STREAM_CHANNEL_CAPACITY: usize = 16;

/// Pre-built failure injected by a mock step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockFailure {
    /// HTTP 429-like rate limit: retryable.
    RateLimited,
    /// HTTP 5xx-like server error: retryable.
    ServerError,
    /// Rejected credentials: not retryable, but a fallback may still help.
    AuthRejected,
    /// Attempt timed out.
    Timeout,
    /// Caller cancelled.
    Cancelled,
    /// Permanent client error (HTTP 400-like).
    NonRetryable,
    /// Capability mismatch.
    Capability,
}

impl MockFailure {
    /// Builds the [`CoreError`] this failure stands for.
    #[must_use]
    pub fn to_error(self, provider: &ProviderId) -> CoreError {
        match self {
            Self::RateLimited => CoreError::provider(
                provider.to_string(),
                format!("provider=`{provider}` mock rate limit (HTTP 429)"),
                true,
            ),
            Self::ServerError => CoreError::provider(
                provider.to_string(),
                format!("provider=`{provider}` mock server error (HTTP 503)"),
                true,
            ),
            Self::AuthRejected => CoreError::authentication(format!(
                "provider=`{provider}` mock rejected the credentials (HTTP 401)"
            )),
            Self::Timeout => CoreError::timeout("mock.complete", Duration::from_millis(1)),
            Self::Cancelled => CoreError::cancelled("mock.complete"),
            Self::NonRetryable => CoreError::provider(
                provider.to_string(),
                format!("provider=`{provider}` mock client error (HTTP 400)"),
                false,
            ),
            Self::Capability => CoreError::capability_unsupported(
                provider.to_string(),
                "mock-model",
                "tool_calling",
            ),
        }
    }
}

/// Description of one call received by a mock adapter.
#[derive(Debug, Clone)]
pub struct RecordedCall {
    /// Provider that was called.
    pub provider: ProviderId,
    /// Wire model id that was resolved.
    pub model: String,
    /// Trace id of the call.
    pub trace_id: TraceId,
    /// Request id of the call.
    pub request_id: RequestId,
    /// Whether the streaming path was used.
    pub streaming: bool,
    /// Number of messages in the request.
    pub message_count: usize,
    /// Whether the request declared tools.
    pub has_tools: bool,
    /// Requested output budget, if any.
    pub max_output_tokens: Option<u32>,
    /// Number of embedding inputs; `0` for chat calls.
    pub input_count: usize,
}

#[derive(Debug, Clone)]
enum MockStep {
    Response {
        content: String,
        finish_reason: FinishReason,
        usage: Option<UsageReport>,
    },
    ToolCalls {
        content: String,
        tool_calls: Vec<ToolCall>,
        usage: Option<UsageReport>,
    },
    Failure(MockFailure),
    Stream {
        chunks: Vec<String>,
        failure: Option<MockFailure>,
    },
    Embeddings {
        vectors: Vec<Vec<f32>>,
        usage: Option<UsageReport>,
    },
}

#[derive(Debug)]
struct MockSettings {
    protocol: ProviderProtocol,
    delay: Duration,
}

#[derive(Debug)]
struct MockInner {
    provider_id: ProviderId,
    settings: Mutex<MockSettings>,
    steps: Mutex<VecDeque<MockStep>>,
    calls: Mutex<Vec<RecordedCall>>,
}

/// Scripted [`ProviderAdapter`] implementation.
#[derive(Debug, Clone)]
pub struct MockAdapter {
    inner: Arc<MockInner>,
}

impl MockAdapter {
    /// Creates a mock bound to `provider_id`, speaking the OpenAI-compatible
    /// protocol by default.
    #[must_use]
    pub fn new(provider_id: ProviderId) -> Self {
        Self {
            inner: Arc::new(MockInner {
                provider_id,
                settings: Mutex::new(MockSettings {
                    protocol: ProviderProtocol::OpenAiCompatible,
                    delay: Duration::ZERO,
                }),
                steps: Mutex::new(VecDeque::new()),
                calls: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Overrides the protocol reported by [`ProviderAdapter::protocol`].
    ///
    /// The registry rejects an adapter whose protocol does not match the provider
    /// configuration, so a mock backing an Anthropic provider must set it.
    #[must_use]
    pub fn with_protocol(self, protocol: ProviderProtocol) -> Self {
        self.update_settings(|settings| settings.protocol = protocol);
        self
    }

    /// Enqueues a successful non-streaming response.
    #[must_use]
    pub fn respond_text(self, content: impl Into<String>) -> Self {
        self.push_step(MockStep::Response {
            content: content.into(),
            finish_reason: FinishReason::Stop,
            usage: None,
        })
    }

    /// Enqueues a successful non-streaming response with usage and finish reason.
    #[must_use]
    pub fn respond_detailed(
        self,
        content: impl Into<String>,
        finish_reason: FinishReason,
        usage: Option<UsageReport>,
    ) -> Self {
        self.push_step(MockStep::Response {
            content: content.into(),
            finish_reason,
            usage,
        })
    }

    /// Enqueues a failure.
    #[must_use]
    pub fn fail(self, failure: MockFailure) -> Self {
        self.push_step(MockStep::Failure(failure))
    }

    /// Enqueues a non-streaming response that requests tool calls.
    ///
    /// The finish reason is [`FinishReason::ToolCalls`], mirroring what a real
    /// adapter reports when it stops to ask for tool execution.
    #[must_use]
    pub fn respond_tool_calls(self, content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        self.push_step(MockStep::ToolCalls {
            content: content.into(),
            tool_calls,
            usage: None,
        })
    }

    /// Enqueues an embedding response holding `vectors`, one per input.
    ///
    /// A count mismatch is reported as a provider error, exactly like the real
    /// adapters do when a vendor ignores part of a batch.
    #[must_use]
    pub fn respond_embeddings(self, vectors: Vec<Vec<f32>>) -> Self {
        self.push_step(MockStep::Embeddings {
            vectors,
            usage: None,
        })
    }

    /// Enqueues an embedding response with an explicit usage report.
    #[must_use]
    pub fn respond_embeddings_with_usage(self, vectors: Vec<Vec<f32>>, usage: UsageReport) -> Self {
        self.push_step(MockStep::Embeddings {
            vectors,
            usage: Some(usage),
        })
    }

    /// Enqueues a successful stream that emits `chunks` as text deltas.
    #[must_use]
    pub fn stream_chunks<I, S>(self, chunks: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.push_step(MockStep::Stream {
            chunks: chunks.into_iter().map(Into::into).collect(),
            failure: None,
        })
    }

    /// Enqueues a stream that emits `chunks` and then fails.
    #[must_use]
    pub fn stream_then_fail<I, S>(self, chunks: I, failure: MockFailure) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.push_step(MockStep::Stream {
            chunks: chunks.into_iter().map(Into::into).collect(),
            failure: Some(failure),
        })
    }

    /// Sets a delay applied before every response (simulates a slow provider).
    #[must_use]
    pub fn with_delay(self, delay: Duration) -> Self {
        self.update_settings(|settings| settings.delay = delay);
        self
    }

    /// Calls received so far, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<RecordedCall> {
        match self.inner.calls.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Number of calls received so far.
    #[must_use]
    pub fn call_count(&self) -> usize {
        match self.inner.calls.lock() {
            Ok(guard) => guard.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }

    /// Removes all recorded calls.
    pub fn clear_calls(&self) {
        match self.inner.calls.lock() {
            Ok(mut guard) => guard.clear(),
            Err(poisoned) => poisoned.into_inner().clear(),
        }
    }

    fn push_step(self, step: MockStep) -> Self {
        match self.inner.steps.lock() {
            Ok(mut steps) => steps.push_back(step),
            Err(poisoned) => poisoned.into_inner().push_back(step),
        }
        self
    }

    fn update_settings(&self, update: impl FnOnce(&mut MockSettings)) {
        match self.inner.settings.lock() {
            Ok(mut settings) => update(&mut settings),
            Err(poisoned) => update(&mut poisoned.into_inner()),
        }
    }

    fn delay(&self) -> Duration {
        match self.inner.settings.lock() {
            Ok(settings) => settings.delay,
            Err(poisoned) => poisoned.into_inner().delay,
        }
    }

    /// Canonical provider id of this mock.
    #[must_use]
    pub fn provider_id(&self) -> &ProviderId {
        &self.inner.provider_id
    }

    fn next_step(&self) -> MockStep {
        let mut steps = match self.inner.steps.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        steps.pop_front().unwrap_or_else(|| MockStep::Response {
            content: DEFAULT_CONTENT.to_owned(),
            finish_reason: FinishReason::Stop,
            usage: None,
        })
    }

    fn record(&self, call: &AdapterCall<'_>, streaming: bool) {
        let entry = RecordedCall {
            provider: call.provider.id.clone(),
            model: call.model.id.clone(),
            trace_id: call.trace.trace_id(),
            request_id: call.trace.request_id(),
            streaming,
            message_count: call.request.messages().len(),
            has_tools: call.request.requires_tool_calling(),
            max_output_tokens: call.request.max_output_tokens(),
            input_count: 0,
        };
        match self.inner.calls.lock() {
            Ok(mut guard) => guard.push(entry),
            Err(poisoned) => poisoned.into_inner().push(entry),
        }
    }

    fn record_embedding(&self, call: &EmbeddingCall<'_>) {
        let entry = RecordedCall {
            provider: call.provider.id.clone(),
            model: call.model.id.clone(),
            trace_id: call.trace.trace_id(),
            request_id: call.trace.request_id(),
            streaming: false,
            message_count: 0,
            has_tools: false,
            max_output_tokens: None,
            input_count: call.request.len(),
        };
        match self.inner.calls.lock() {
            Ok(mut guard) => guard.push(entry),
            Err(poisoned) => poisoned.into_inner().push(entry),
        }
    }

    async fn wait_delay(&self) {
        let delay = self.delay();
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }
}

/// UTF-8 byte length of a string, saturating on absurd sizes.
fn byte_len(text: &str) -> u64 {
    u64::try_from(text.len()).unwrap_or(u64::MAX)
}

/// Rough request body size of a call, so tests can assert traffic accounting.
fn request_size(call: &AdapterCall<'_>) -> u64 {
    let mut bytes = 0_u64;
    for message in call.request.messages() {
        for part in message.parts() {
            match part {
                ContentPart::Text(text) => bytes = bytes.saturating_add(byte_len(text)),
                ContentPart::Image { url } => bytes = bytes.saturating_add(byte_len(url)),
            }
        }
    }
    for tool in call.request.tools() {
        if let Ok(schema) = serde_json::to_string(tool.parameters()) {
            bytes = bytes.saturating_add(byte_len(&schema));
        }
    }
    bytes
}

impl ProviderAdapter for MockAdapter {
    fn protocol(&self) -> ProviderProtocol {
        match self.inner.settings.lock() {
            Ok(settings) => settings.protocol,
            Err(poisoned) => poisoned.into_inner().protocol,
        }
    }

    fn label(&self) -> &str {
        "mock"
    }

    fn complete<'a>(&'a self, call: AdapterCall<'a>) -> BoxFuture<'a, CoreResult<AiResponse>> {
        Box::pin(async move {
            self.record(&call, false);
            self.wait_delay().await;

            let provider_id = call.provider.id.clone();
            let model_id = call.model.id.clone();
            let request_bytes = request_size(&call);
            match self.next_step() {
                MockStep::Response {
                    content,
                    finish_reason,
                    usage,
                } => {
                    let response_bytes = byte_len(&content);
                    let mut response = AiResponse::new(provider_id, model_id)
                        .with_content(content)
                        .with_finish_reason(finish_reason)
                        .with_transport(TransportStats {
                            request_bytes,
                            response_bytes,
                        });
                    if let Some(usage) = usage {
                        response = response.with_usage(usage);
                    }
                    Ok(response)
                }
                MockStep::Failure(failure) => Err(failure.to_error(&provider_id)),
                MockStep::ToolCalls {
                    content,
                    tool_calls,
                    usage,
                } => {
                    let response_bytes = byte_len(&content)
                        + tool_calls
                            .iter()
                            .map(|call| byte_len(&call.arguments))
                            .sum::<u64>();
                    let mut response = AiResponse::new(provider_id, model_id)
                        .with_content(content)
                        .with_tool_calls(tool_calls)
                        .with_finish_reason(FinishReason::ToolCalls)
                        .with_transport(TransportStats {
                            request_bytes,
                            response_bytes,
                        });
                    if let Some(usage) = usage {
                        response = response.with_usage(usage);
                    }
                    Ok(response)
                }
                MockStep::Stream { chunks, failure } => {
                    if let Some(failure) = failure {
                        return Err(failure.to_error(&provider_id));
                    }
                    let content = chunks.concat();
                    Ok(AiResponse::new(provider_id, model_id)
                        .with_content(content.clone())
                        .with_finish_reason(FinishReason::Stop)
                        .with_transport(TransportStats {
                            request_bytes,
                            response_bytes: byte_len(&content),
                        }))
                }
                MockStep::Embeddings { .. } => Err(CoreError::invalid_argument(
                    "mock.step",
                    "scripted an embeddings step on the chat path",
                )),
            }
        })
    }

    fn embed<'a>(
        &'a self,
        call: EmbeddingCall<'a>,
    ) -> BoxFuture<'a, CoreResult<EmbeddingResponse>> {
        Box::pin(async move {
            self.record_embedding(&call);
            self.wait_delay().await;

            let provider_id = call.provider.id.clone();
            let model_id = call.model.id.clone();
            let inputs = call.request.len();
            let request_bytes = call
                .request
                .inputs()
                .iter()
                .map(|input| byte_len(input))
                .sum::<u64>();

            match self.next_step() {
                MockStep::Embeddings { vectors, usage } => {
                    if vectors.len() != inputs {
                        return Err(CoreError::provider(
                            provider_id.to_string(),
                            format!(
                                "provider=`{provider_id}` model=`{model_id}` returned {} vectors \
                                 for {inputs} inputs",
                                vectors.len()
                            ),
                            false,
                        ));
                    }
                    let response_bytes = vectors.iter().fold(0_u64, |total, vector| {
                        total.saturating_add(
                            u64::try_from(vector.len()).unwrap_or(0).saturating_mul(4),
                        )
                    });
                    let mut response = EmbeddingResponse::new(provider_id, model_id)
                        .with_vectors(vectors)
                        .with_transport(TransportStats {
                            request_bytes,
                            response_bytes,
                        });
                    if let Some(usage) = usage {
                        response = response.with_usage(usage);
                    }
                    Ok(response)
                }
                MockStep::Failure(failure) => Err(failure.to_error(&provider_id)),
                _ => Err(CoreError::invalid_argument(
                    "mock.step",
                    "scripted a chat step on the embeddings path",
                )),
            }
        })
    }

    fn stream<'a>(&'a self, call: AdapterCall<'a>) -> BoxFuture<'a, CoreResult<AiStream>> {
        Box::pin(async move {
            self.record(&call, true);
            self.wait_delay().await;

            let provider_id = call.provider.id.clone();
            let model_id = call.model.id.clone();
            let request_bytes = request_size(&call);
            let step = self.next_step();
            let (chunks, failure, tool_calls, finish_reason) = match step {
                MockStep::Stream { chunks, failure } => {
                    (chunks, failure, Vec::new(), FinishReason::Stop)
                }
                MockStep::Response {
                    content,
                    finish_reason,
                    ..
                } => (vec![content], None, Vec::new(), finish_reason),
                MockStep::ToolCalls {
                    content,
                    tool_calls,
                    ..
                } => (vec![content], None, tool_calls, FinishReason::ToolCalls),
                MockStep::Embeddings { .. } => {
                    return Err(CoreError::invalid_argument(
                        "mock.step",
                        "scripted an embeddings step on the streaming path",
                    ))
                }
                MockStep::Failure(failure) => return Err(failure.to_error(&provider_id)),
            };

            let (sender, receiver) = mpsc::channel(STREAM_CHANNEL_CAPACITY);
            let stream = AiStream::new(provider_id.clone(), model_id.clone(), receiver);

            tokio::spawn(async move {
                if sender
                    .send(Ok(StreamEvent::Started {
                        provider_id: provider_id.clone(),
                        model_id,
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
                let mut bytes = 0_u64;
                for chunk in chunks {
                    bytes += u64::try_from(chunk.len()).unwrap_or(0);
                    if sender
                        .send(Ok(StreamEvent::TextDelta { text: chunk }))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                if let Some(failure) = failure {
                    let _ = sender.send(Err(failure.to_error(&provider_id))).await;
                    return;
                }
                for (index, call) in tool_calls.into_iter().enumerate() {
                    bytes += u64::try_from(call.arguments.len()).unwrap_or(0);
                    if sender
                        .send(Ok(StreamEvent::ToolCallDelta {
                            index: u16::try_from(index).unwrap_or(u16::MAX),
                            id: Some(call.id),
                            name: Some(call.name),
                            arguments: call.arguments,
                        }))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                let usage = UsageReport {
                    prompt_tokens: Some(1),
                    completion_tokens: Some(u32::try_from(bytes).unwrap_or(u32::MAX)),
                    total_tokens: None,
                    reasoning_tokens: None,
                    cached_tokens: None,
                    raw: None,
                };
                let _ = sender
                    .send(Ok(StreamEvent::Finished {
                        finish_reason,
                        usage: Some(usage),
                        transport: TransportStats {
                            request_bytes,
                            response_bytes: bytes,
                        },
                    }))
                    .await;
            });

            Ok(stream)
        })
    }
}
