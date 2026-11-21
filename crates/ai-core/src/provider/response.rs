//! Unified (non-streaming and streaming) response contract.
//!
//! Adapters normalize vendor payloads into [`AiResponse`] / [`StreamEvent`], so
//! the rest of the framework never sees OpenAI's `choices[0].delta` or
//! Anthropic's `content_block_delta`. The raw usage object is preserved verbatim
//! ([`UsageReport::raw`]) because it is the input the Phase 3 usage system needs.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_core::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::ids::{RequestId, TraceId};
use crate::provider::config::ProviderId;
use crate::CoreResult;

/// Why the model stopped generating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural end of turn.
    #[default]
    Stop,
    /// Output token budget reached.
    Length,
    /// The model wants to call a tool.
    ToolCalls,
    /// Content filtering stopped the generation.
    ContentFilter,
    /// The caller cancelled the request mid-stream.
    Cancelled,
    /// The stream ended prematurely (network drop, provider `incomplete`).
    Incomplete,
    /// Provider-specific reason this crate does not model yet.
    Unknown,
}

impl FinishReason {
    /// Stable lowercase label used in logs and UI payloads.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::ToolCalls => "tool_calls",
            Self::ContentFilter => "content_filter",
            Self::Cancelled => "cancelled",
            Self::Incomplete => "incomplete",
            Self::Unknown => "unknown",
        }
    }

    /// Whether the response is a complete answer (not truncated or interrupted).
    #[must_use]
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Stop | Self::ToolCalls)
    }
}

impl std::fmt::Display for FinishReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Outcome classification of one provider attempt.
///
/// The status describes the *attempt*, not the whole logical request: a request
/// that succeeded on its second try produces one `Failed` row and one
/// `Degraded` row in the usage store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RequestStatus {
    /// The attempt succeeded on its first try.
    #[default]
    Success,
    /// The attempt succeeded, but only after retries or a fallback.
    Degraded,
    /// The attempt failed with a non-retryable provider/configuration error.
    Failed,
    /// The caller cancelled the attempt.
    Cancelled,
    /// The attempt exceeded its deadline.
    Timeout,
}

impl RequestStatus {
    /// Stable lowercase label used in storage, logs and UI payloads.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
        }
    }

    /// Parses the label produced by [`Self::as_str`].
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "success" => Some(Self::Success),
            "degraded" => Some(Self::Degraded),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "timeout" => Some(Self::Timeout),
            _ => None,
        }
    }

    /// Whether the attempt produced usable output.
    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Success | Self::Degraded)
    }
}

impl std::fmt::Display for RequestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Locally estimated token counts, used only when the provider reported nothing.
///
/// Carrying numbers instead of text keeps prompt/response content out of the
/// accounting path: the estimator runs in the router, the observer only ever
/// sees counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EstimatedTokens {
    /// Estimated input tokens.
    pub input: u32,
    /// Estimated output tokens.
    pub output: u32,
}

impl EstimatedTokens {
    /// Builds an estimate from its two components.
    #[must_use]
    pub const fn new(input: u32, output: u32) -> Self {
        Self { input, output }
    }
}

/// How an [`AiStream`] ended, as observed by its completion hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamTermination {
    /// The terminal [`StreamEvent::Finished`] event was observed.
    Completed,
    /// An `Err` item was observed before the terminal event.
    Failed,
    /// The stream ended without a terminal event (dropped early or closed).
    Aborted,
}

/// Summary handed to the completion hook installed by the router.
///
/// It carries counts and metadata only — never the generated text — so the
/// usage subsystem can persist traffic and token statistics without reading
/// conversation content.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamSummary {
    /// Provider that served the stream.
    pub provider_id: ProviderId,
    /// Wire model id.
    pub model_id: String,
    /// Final usage reported by the provider, when available.
    pub usage: Option<UsageReport>,
    /// Transport byte counters reported by the terminal event.
    pub transport: TransportStats,
    /// Finish reason of the terminal event, when reached.
    pub finish_reason: Option<FinishReason>,
    /// How the stream ended.
    pub termination: StreamTermination,
    /// Tokens estimated from the accumulated text (never provider-reported).
    pub estimated_output_tokens: u32,
}

/// Callback invoked once when a stream ends, errors or is dropped.
///
/// The newtype exists only so [`AiStream`] can keep its derived `Debug`:
/// closures are not `Debug`.
pub(crate) struct StreamCompletionHook(Box<dyn FnOnce(StreamSummary) + Send + 'static>);

impl StreamCompletionHook {
    pub(crate) fn new(hook: Box<dyn FnOnce(StreamSummary) + Send + 'static>) -> Self {
        Self(hook)
    }

    fn call(self, summary: StreamSummary) {
        (self.0)(summary);
    }
}

impl std::fmt::Debug for StreamCompletionHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamCompletionHook")
    }
}

/// Tool invocation requested by the model.
///
/// Arguments are kept as the raw JSON string produced by the provider: Phase 2
/// does not validate or execute them (that is the tool subsystem's job).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned call id (needed to submit the result later).
    pub id: String,
    /// Function name.
    pub name: String,
    /// Raw JSON argument string, as produced by the model.
    pub arguments: String,
}

/// Token accounting reported by the provider.
///
/// Fields are `Option` because providers disagree on what they report; the
/// untouched object is kept in [`UsageReport::raw`] for forward compatibility and
/// for the Phase 3 usage accounting system.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageReport {
    /// Input/prompt tokens.
    pub prompt_tokens: Option<u32>,
    /// Generated tokens.
    pub completion_tokens: Option<u32>,
    /// Total tokens, when reported or derivable.
    pub total_tokens: Option<u32>,
    /// Tokens spent on reasoning/thinking, when reported.
    pub reasoning_tokens: Option<u32>,
    /// Tokens served from a provider cache, when reported.
    pub cached_tokens: Option<u32>,
    /// Provider-native usage object (already free of credentials).
    pub raw: Option<serde_json::Value>,
}

impl UsageReport {
    /// Whether the report carries no information at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.prompt_tokens.is_none()
            && self.completion_tokens.is_none()
            && self.total_tokens.is_none()
            && self.reasoning_tokens.is_none()
            && self.cached_tokens.is_none()
            && self.raw.is_none()
    }
}

/// Byte counters describing the transport work of one call.
///
/// Exposed for the Phase 3 accounting system: it can be summed per
/// provider/model without re-reading any payload.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportStats {
    /// Serialized request body size in bytes.
    pub request_bytes: u64,
    /// Raw response bytes received (including SSE framing).
    pub response_bytes: u64,
}

/// A completed (non-streaming) generation.
#[derive(Debug, Clone)]
pub struct AiResponse {
    provider_id: ProviderId,
    model_id: String,
    trace_id: Option<TraceId>,
    request_id: Option<RequestId>,
    content: String,
    reasoning: Option<String>,
    tool_calls: Vec<ToolCall>,
    finish_reason: FinishReason,
    usage: Option<UsageReport>,
    transport: TransportStats,
    latency: Duration,
    attempts: u32,
}

impl AiResponse {
    /// Builds an empty response for `provider_id`/`model_id`.
    ///
    /// Adapters fill it with the [`AiResponse::with_content`] family; the router
    /// then attaches correlation ids, attempt count and latency.
    #[must_use]
    pub fn new(provider_id: ProviderId, model_id: impl Into<String>) -> Self {
        Self {
            provider_id,
            model_id: model_id.into(),
            trace_id: None,
            request_id: None,
            content: String::new(),
            reasoning: None,
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
            transport: TransportStats::default(),
            latency: Duration::ZERO,
            attempts: 1,
        }
    }

    /// Sets the generated text.
    #[must_use]
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    /// Sets the reasoning/thinking text.
    #[must_use]
    pub fn with_reasoning(mut self, reasoning: impl Into<String>) -> Self {
        self.reasoning = Some(reasoning.into());
        self
    }

    /// Sets the requested tool calls.
    #[must_use]
    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    /// Sets the finish reason.
    #[must_use]
    pub fn with_finish_reason(mut self, finish_reason: FinishReason) -> Self {
        self.finish_reason = finish_reason;
        self
    }

    /// Sets the usage report.
    #[must_use]
    pub fn with_usage(mut self, usage: UsageReport) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Sets the transport counters.
    #[must_use]
    pub fn with_transport(mut self, transport: TransportStats) -> Self {
        self.transport = transport;
        self
    }

    /// Attaches the trace/request identifiers of the originating call.
    #[must_use]
    pub fn with_correlation(mut self, trace_id: TraceId, request_id: RequestId) -> Self {
        self.trace_id = Some(trace_id);
        self.request_id = Some(request_id);
        self
    }

    /// Provider that produced the response.
    #[must_use]
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    /// Wire model id that produced the response.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Trace identifier of the originating call.
    #[must_use]
    pub fn trace_id(&self) -> Option<TraceId> {
        self.trace_id
    }

    /// Request identifier of the upstream call.
    #[must_use]
    pub fn request_id(&self) -> Option<RequestId> {
        self.request_id
    }

    /// Generated text.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Reasoning/thinking text, when the provider returned it.
    #[must_use]
    pub fn reasoning(&self) -> Option<&str> {
        self.reasoning.as_deref()
    }

    /// Tool calls requested by the model.
    #[must_use]
    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.tool_calls
    }

    /// Why the model stopped.
    #[must_use]
    pub fn finish_reason(&self) -> FinishReason {
        self.finish_reason
    }

    /// Token accounting, when reported.
    #[must_use]
    pub fn usage(&self) -> Option<&UsageReport> {
        self.usage.as_ref()
    }

    /// Transport byte counters.
    #[must_use]
    pub fn transport(&self) -> TransportStats {
        self.transport
    }

    /// Wall-clock duration of the successful attempt.
    #[must_use]
    pub fn latency(&self) -> Duration {
        self.latency
    }

    /// How many upstream attempts were consumed (1 = first try succeeded).
    #[must_use]
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub(crate) fn set_attempts(&mut self, attempts: u32) {
        self.attempts = attempts;
    }

    pub(crate) fn set_latency(&mut self, latency: Duration) {
        self.latency = latency;
    }

    pub(crate) fn set_correlation(&mut self, trace_id: TraceId, request_id: RequestId) {
        self.trace_id = Some(trace_id);
        self.request_id = Some(request_id);
    }
}

/// A completed embedding call: one vector per input text, in input order.
///
/// Vectors are plain `f32` rows so the host decides how to store them; the
/// framework never assumes a metric or an index. A provider that returns a
/// different number of vectors than inputs is rejected by the adapter (an
/// alignment bug would otherwise silently pair the wrong text with a vector).
#[derive(Debug, Clone)]
pub struct EmbeddingResponse {
    provider_id: ProviderId,
    model_id: String,
    trace_id: Option<TraceId>,
    request_id: Option<RequestId>,
    vectors: Vec<Vec<f32>>,
    usage: Option<UsageReport>,
    transport: TransportStats,
    latency: Duration,
    attempts: u32,
}

impl EmbeddingResponse {
    /// Builds an empty response for `provider_id`/`model_id`.
    #[must_use]
    pub fn new(provider_id: ProviderId, model_id: impl Into<String>) -> Self {
        Self {
            provider_id,
            model_id: model_id.into(),
            trace_id: None,
            request_id: None,
            vectors: Vec::new(),
            usage: None,
            transport: TransportStats::default(),
            latency: Duration::ZERO,
            attempts: 1,
        }
    }

    /// Sets the returned vectors.
    #[must_use]
    pub fn with_vectors(mut self, vectors: Vec<Vec<f32>>) -> Self {
        self.vectors = vectors;
        self
    }

    /// Sets the usage report.
    #[must_use]
    pub fn with_usage(mut self, usage: UsageReport) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Sets the transport counters.
    #[must_use]
    pub fn with_transport(mut self, transport: TransportStats) -> Self {
        self.transport = transport;
        self
    }

    /// Attaches the trace/request identifiers of the originating call.
    #[must_use]
    pub fn with_correlation(mut self, trace_id: TraceId, request_id: RequestId) -> Self {
        self.trace_id = Some(trace_id);
        self.request_id = Some(request_id);
        self
    }

    /// Provider that produced the vectors.
    #[must_use]
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    /// Wire model id that produced the vectors.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Trace identifier of the originating call.
    #[must_use]
    pub fn trace_id(&self) -> Option<TraceId> {
        self.trace_id
    }

    /// Request identifier of the upstream call.
    #[must_use]
    pub fn request_id(&self) -> Option<RequestId> {
        self.request_id
    }

    /// One vector per input, in input order.
    #[must_use]
    pub fn vectors(&self) -> &[Vec<f32>] {
        &self.vectors
    }

    /// Number of returned vectors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    /// Whether no vector was returned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    /// Width of the first vector, `None` when there is no vector.
    #[must_use]
    pub fn dimensions(&self) -> Option<usize> {
        self.vectors.first().map(Vec::len)
    }

    /// Token accounting, when reported. Embedding providers usually report
    /// input tokens only.
    #[must_use]
    pub fn usage(&self) -> Option<&UsageReport> {
        self.usage.as_ref()
    }

    /// Transport byte counters.
    #[must_use]
    pub fn transport(&self) -> TransportStats {
        self.transport
    }

    /// Wall-clock duration of the successful attempt.
    #[must_use]
    pub fn latency(&self) -> Duration {
        self.latency
    }

    /// How many upstream attempts were consumed (1 = first try succeeded).
    #[must_use]
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub(crate) fn set_attempts(&mut self, attempts: u32) {
        self.attempts = attempts;
    }

    pub(crate) fn set_latency(&mut self, latency: Duration) {
        self.latency = latency;
    }

    pub(crate) fn set_correlation(&mut self, trace_id: TraceId, request_id: RequestId) {
        self.trace_id = Some(trace_id);
        self.request_id = Some(request_id);
    }
}

/// One event of a streaming response.
///
/// The order is guaranteed: [`StreamEvent::Started`] first, then any number of
/// deltas, then exactly one terminal event (either
/// [`StreamEvent::Finished`] or an `Err` item after which the stream ends).
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Stream opened; carries the resolved provider/model.
    Started {
        /// Provider serving the stream.
        provider_id: ProviderId,
        /// Wire model id.
        model_id: String,
    },
    /// Incremental answer text.
    TextDelta {
        /// Newly generated text.
        text: String,
    },
    /// Incremental reasoning/thinking text.
    ReasoningDelta {
        /// Newly generated reasoning text.
        text: String,
    },
    /// Incremental tool-call data (arguments arrive in fragments).
    ToolCallDelta {
        /// Index of the tool call within the response.
        index: u16,
        /// Call id, present on the first fragment.
        id: Option<String>,
        /// Function name, present on the first fragment.
        name: Option<String>,
        /// Argument fragment.
        arguments: String,
    },
    /// Token accounting reported during the stream (e.g. final chunk).
    Usage(UsageReport),
    /// Terminal event of a successful stream.
    Finished {
        /// Why the model stopped.
        finish_reason: FinishReason,
        /// Final usage report, when available.
        usage: Option<UsageReport>,
        /// Transport byte counters.
        transport: TransportStats,
    },
}

impl StreamEvent {
    /// Text carried by this event, if any.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::TextDelta { text } => Some(text),
            _ => None,
        }
    }

    /// Whether this event terminates the stream.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Finished { .. })
    }
}

/// Result of draining a stream to its end.
#[derive(Debug, Clone, Default)]
pub struct StreamOutcome {
    /// Accumulated answer text.
    pub text: String,
    /// Accumulated reasoning text.
    pub reasoning: String,
    /// Finish reason of the terminal event, when the stream finished cleanly.
    pub finish_reason: Option<FinishReason>,
    /// Final usage report.
    pub usage: Option<UsageReport>,
    /// Transport counters.
    pub transport: TransportStats,
}

/// An in-progress streaming response.
///
/// `AiStream` is a [`Stream`] of [`StreamEvent`]s that also accumulates the
/// generated text, so a consumer that hits an error or drops the handle early can
/// still show what was received ([`AiStream::partial_text`]). Dropping the stream
/// closes the channel and makes the producer task exit, which releases the HTTP
/// connection.
#[derive(Debug)]
pub struct AiStream {
    provider_id: ProviderId,
    model_id: String,
    trace_id: Option<TraceId>,
    request_id: Option<RequestId>,
    receiver: mpsc::Receiver<CoreResult<StreamEvent>>,
    text: String,
    reasoning: String,
    usage: Option<UsageReport>,
    finish_reason: Option<FinishReason>,
    transport: TransportStats,
    finished: bool,
    completion: Option<StreamCompletionHook>,
}

impl AiStream {
    /// Wraps a receiver into a stream.
    ///
    /// Adapters usually create the channel with [`tokio::sync::mpsc::channel`] and
    /// return `AiStream::new(...)`; the producer half is moved into a task that
    /// reads the provider stream.
    #[must_use]
    pub fn new(
        provider_id: ProviderId,
        model_id: impl Into<String>,
        receiver: mpsc::Receiver<CoreResult<StreamEvent>>,
    ) -> Self {
        Self {
            provider_id,
            model_id: model_id.into(),
            trace_id: None,
            request_id: None,
            receiver,
            text: String::new(),
            reasoning: String::new(),
            usage: None,
            finish_reason: None,
            transport: TransportStats::default(),
            finished: false,
            completion: None,
        }
    }

    pub(crate) fn set_correlation(&mut self, trace_id: TraceId, request_id: RequestId) {
        self.trace_id = Some(trace_id);
        self.request_id = Some(request_id);
    }

    /// Installs the hook invoked exactly once when the stream ends.
    ///
    /// The hook also runs when the handle is dropped without being drained, so
    /// abandoned streams still settle their usage and traffic counters.
    pub(crate) fn set_completion_hook(
        &mut self,
        hook: Box<dyn FnOnce(StreamSummary) + Send + 'static>,
    ) {
        self.completion = Some(StreamCompletionHook::new(hook));
    }

    /// Fires the completion hook, if it is still armed.
    fn settle(&mut self, termination: StreamTermination) {
        let Some(hook) = self.completion.take() else {
            return;
        };
        hook.call(StreamSummary {
            provider_id: self.provider_id.clone(),
            model_id: self.model_id.clone(),
            usage: self.usage.clone(),
            transport: self.transport,
            finish_reason: self.finish_reason,
            termination,
            estimated_output_tokens: crate::provider::capabilities::estimate_text_tokens(
                &self.text,
            ),
        });
    }

    /// Provider serving the stream.
    #[must_use]
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    /// Wire model id.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Trace identifier of the originating call.
    #[must_use]
    pub fn trace_id(&self) -> Option<TraceId> {
        self.trace_id
    }

    /// Request identifier of the upstream call.
    #[must_use]
    pub fn request_id(&self) -> Option<RequestId> {
        self.request_id
    }

    /// Text received so far.
    #[must_use]
    pub fn partial_text(&self) -> &str {
        &self.text
    }

    /// Reasoning text received so far.
    #[must_use]
    pub fn partial_reasoning(&self) -> &str {
        &self.reasoning
    }

    /// Usage reported so far.
    #[must_use]
    pub fn usage(&self) -> Option<&UsageReport> {
        self.usage.as_ref()
    }

    /// Finish reason, once the terminal event was observed.
    #[must_use]
    pub fn finish_reason(&self) -> Option<FinishReason> {
        self.finish_reason
    }

    /// Transport counters reported by the terminal event.
    #[must_use]
    pub fn transport(&self) -> TransportStats {
        self.transport
    }

    /// Whether a terminal event (or an error) was observed.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Drains the stream and returns the accumulated result.
    ///
    /// # Errors
    ///
    /// Returns the first error item (timeout, cancellation, provider failure)
    /// after having kept the partial output internally; call
    /// [`AiStream::partial_text`] on a clone-free path by inspecting the error and
    /// re-reading the handle is not possible after `finish` consumes it, so
    /// callers that need partial output on failure should poll the stream manually.
    pub async fn finish(mut self) -> CoreResult<StreamOutcome> {
        loop {
            let next = std::future::poll_fn(|cx| Pin::new(&mut self).poll_next(cx)).await;
            match next {
                None => break,
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    return Err(error);
                }
            }
        }
        Ok(StreamOutcome {
            text: std::mem::take(&mut self.text),
            reasoning: std::mem::take(&mut self.reasoning),
            finish_reason: self.finish_reason,
            usage: self.usage.take(),
            transport: self.transport,
        })
    }

    fn absorb(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::TextDelta { text } => self.text.push_str(text),
            StreamEvent::ReasoningDelta { text } => self.reasoning.push_str(text),
            StreamEvent::Usage(usage) => self.usage = Some(usage.clone()),
            StreamEvent::Finished {
                finish_reason,
                usage,
                transport,
            } => {
                self.finish_reason = Some(*finish_reason);
                if usage.is_some() {
                    self.usage = usage.clone();
                }
                self.transport = *transport;
                self.finished = true;
            }
            StreamEvent::Started { .. } | StreamEvent::ToolCallDelta { .. } => {}
        }
    }
}

impl Stream for AiStream {
    type Item = CoreResult<StreamEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.receiver.poll_recv(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => {
                this.finished = true;
                this.settle(StreamTermination::Aborted);
                Poll::Ready(None)
            }
            Poll::Ready(Some(item)) => {
                match &item {
                    Ok(event) => {
                        this.absorb(event);
                        if event.is_terminal() {
                            this.settle(StreamTermination::Completed);
                        }
                    }
                    Err(_) => {
                        this.finished = true;
                        this.settle(StreamTermination::Failed);
                    }
                }
                Poll::Ready(Some(item))
            }
        }
    }
}

impl Drop for AiStream {
    fn drop(&mut self) {
        // A consumer that stops polling early must still produce a usage row:
        // the hook is armed until the first settlement, so this is the last
        // chance to account for the bytes and tokens that were already spent.
        self.settle(StreamTermination::Aborted);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn channel_stream() -> (mpsc::Sender<CoreResult<StreamEvent>>, AiStream) {
        let (sender, receiver) = mpsc::channel(8);
        let stream = AiStream::new(ProviderId::new("mock").unwrap(), "mock-1", receiver);
        (sender, stream)
    }

    #[tokio::test]
    async fn stream_accumulates_text_and_finish_state() {
        let (sender, stream) = channel_stream();
        sender
            .send(Ok(StreamEvent::Started {
                provider_id: ProviderId::new("mock").unwrap(),
                model_id: "mock-1".to_owned(),
            }))
            .await
            .unwrap();
        sender
            .send(Ok(StreamEvent::TextDelta {
                text: "hello ".to_owned(),
            }))
            .await
            .unwrap();
        sender
            .send(Ok(StreamEvent::TextDelta {
                text: "world".to_owned(),
            }))
            .await
            .unwrap();
        sender
            .send(Ok(StreamEvent::Finished {
                finish_reason: FinishReason::Stop,
                usage: Some(UsageReport {
                    completion_tokens: Some(2),
                    ..UsageReport::default()
                }),
                transport: TransportStats {
                    request_bytes: 10,
                    response_bytes: 20,
                },
            }))
            .await
            .unwrap();
        drop(sender);

        let outcome = stream.finish().await.unwrap();
        assert_eq!(outcome.text, "hello world");
        assert_eq!(outcome.finish_reason, Some(FinishReason::Stop));
        assert_eq!(
            outcome.usage.and_then(|usage| usage.completion_tokens),
            Some(2)
        );
        assert_eq!(outcome.transport.response_bytes, 20);
    }

    #[tokio::test]
    async fn stream_surfaces_errors_after_partial_output() {
        let (sender, mut stream) = channel_stream();
        sender
            .send(Ok(StreamEvent::TextDelta {
                text: "partial".to_owned(),
            }))
            .await
            .unwrap();
        sender
            .send(Err(crate::CoreError::cancelled("stream")))
            .await
            .unwrap();
        drop(sender);

        let mut error = None;
        while let Some(item) = std::future::poll_fn(|cx| Pin::new(&mut stream).poll_next(cx)).await
        {
            if let Err(value) = item {
                error = Some(value);
                break;
            }
        }
        assert!(error.map(|error| error.is_cancelled()).unwrap_or(false));
        assert_eq!(stream.partial_text(), "partial");
        assert!(stream.is_finished());
    }

    #[test]
    fn finish_reasons_are_classified() {
        assert!(FinishReason::Stop.is_complete());
        assert!(FinishReason::ToolCalls.is_complete());
        assert!(!FinishReason::Length.is_complete());
        assert!(!FinishReason::Cancelled.is_complete());
        assert_eq!(FinishReason::ContentFilter.to_string(), "content_filter");
    }
}
