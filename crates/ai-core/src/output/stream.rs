//! Streaming output: provider events → [`CoreOutputEvent`]s with partial capture.
//!
//! The adapter must be able to cancel mid-stream, must never lose what was
//! already produced, and must always end with exactly one terminal event. Those
//! three properties live here, not in the host.

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;

use crate::async_utils::CancelToken;
use crate::error::CoreError;
use crate::output::event::{CoreOutputEvent, OutputError, OutputOutcome, OutputToolCall};
use crate::output::structured::{parse_structured, StructuredOutput};
use crate::provider::response::{AiStream, FinishReason, StreamEvent, UsageReport};
use crate::provider::types::ResponseFormat;

/// Tool call being assembled from incremental fragments.
#[derive(Debug)]
struct ToolCallAssembly {
    index: u16,
    call_id: String,
    name: String,
    arguments: String,
    started: bool,
}

/// Event stream produced by [`OutputEngine::stream`](crate::output::OutputEngine::stream).
///
/// Drop safety: dropping the stream early still settles usage accounting through
/// the inner [`AiStream`], and the caller can read the partial state with
/// [`OutputStream::partial_text`] before dropping.
#[derive(Debug)]
pub struct OutputStream {
    inner: AiStream,
    cancel: CancelToken,
    format: ResponseFormat,
    pending: VecDeque<CoreOutputEvent>,
    text: String,
    reasoning: String,
    tool_calls: Vec<ToolCallAssembly>,
    structured: Option<StructuredOutput>,
    usage: Option<UsageReport>,
    finish_reason: Option<FinishReason>,
    interrupted: bool,
    error: Option<OutputError>,
    completed: bool,
    closed: bool,
}

impl OutputStream {
    /// Wraps a provider stream, synthesizing the `Started` event.
    pub(crate) fn new(inner: AiStream, cancel: CancelToken, format: ResponseFormat) -> Self {
        let mut pending = VecDeque::new();
        pending.push_back(CoreOutputEvent::Started {
            request_id: inner.request_id(),
            trace_id: inner.trace_id(),
            provider_id: inner.provider_id().clone(),
            model_id: inner.model_id().to_owned(),
            streaming: true,
        });
        Self {
            inner,
            cancel,
            format,
            pending,
            text: String::new(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
            structured: None,
            usage: None,
            finish_reason: None,
            interrupted: false,
            error: None,
            completed: false,
            closed: false,
        }
    }

    /// Text accumulated so far, readable at any point (including after an error).
    #[must_use]
    pub fn partial_text(&self) -> &str {
        &self.text
    }

    /// Reasoning accumulated so far.
    #[must_use]
    pub fn partial_reasoning(&self) -> &str {
        &self.reasoning
    }

    /// Whether a terminal event has been produced.
    #[must_use]
    pub fn is_completed(&self) -> bool {
        self.completed
    }

    /// Consumes the stream, forwarding every event to `sink`.
    ///
    /// The sink sees exactly the same sequence a `Stream` consumer would; the
    /// returned outcome is built from the accumulated state, so it always
    /// preserves partial output.
    pub async fn run<F>(mut self, mut sink: F) -> OutputOutcome
    where
        F: FnMut(&CoreOutputEvent),
    {
        while let Some(event) = std::future::poll_fn(|cx| Pin::new(&mut self).poll_next(cx)).await {
            let terminal = event.is_terminal();
            sink(&event);
            if terminal {
                break;
            }
        }
        self.into_outcome()
    }

    /// Consumes the stream without a sink and returns the final outcome.
    pub async fn drain(self) -> OutputOutcome {
        self.run(|_| {}).await
    }

    /// Converts the accumulated state into an outcome.
    #[must_use]
    pub fn into_outcome(self) -> OutputOutcome {
        OutputOutcome {
            text: self.text,
            reasoning: self.reasoning,
            structured: self.structured,
            tool_calls: self
                .tool_calls
                .into_iter()
                .map(|call| OutputToolCall {
                    index: call.index,
                    call_id: call.call_id,
                    name: call.name,
                    arguments: call.arguments,
                })
                .collect(),
            usage: self.usage,
            finish_reason: self.finish_reason,
            interrupted: self.interrupted,
            error: self.error,
        }
    }

    fn absorb(&mut self, event: StreamEvent) {
        match event {
            // The provider's own start event is redundant: `new` already emitted
            // a `Started` with the same correlation data.
            StreamEvent::Started { .. } => {}
            StreamEvent::TextDelta { text } => {
                self.text.push_str(&text);
                self.pending.push_back(CoreOutputEvent::TextDelta { text });
            }
            StreamEvent::ReasoningDelta { text } => {
                self.reasoning.push_str(&text);
                self.pending
                    .push_back(CoreOutputEvent::ReasoningDelta { text });
            }
            StreamEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } => self.absorb_tool_delta(index, id, name, arguments),
            StreamEvent::Usage(usage) => {
                self.usage = Some(usage.clone());
                self.pending
                    .push_back(CoreOutputEvent::UsageUpdated { usage });
            }
            StreamEvent::Finished {
                finish_reason,
                usage,
                ..
            } => {
                self.finish_reason = Some(finish_reason);
                if let Some(usage) = usage {
                    self.usage = Some(usage.clone());
                    self.pending
                        .push_back(CoreOutputEvent::UsageUpdated { usage });
                }
                self.finish_successfully();
            }
        }
    }

    fn absorb_tool_delta(
        &mut self,
        index: u16,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    ) {
        let position = match self.tool_calls.iter().position(|call| call.index == index) {
            Some(position) => position,
            None => {
                self.tool_calls.push(ToolCallAssembly {
                    index,
                    call_id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                    started: false,
                });
                self.tool_calls.len() - 1
            }
        };

        let call = &mut self.tool_calls[position];
        if let Some(id) = id {
            if call.call_id.is_empty() {
                call.call_id = id;
            }
        }
        if let Some(name) = name {
            if call.name.is_empty() {
                call.name = name;
            }
        }
        call.arguments.push_str(&arguments);
        let started_now = !call.started;
        call.started = true;
        let call_id = call.call_id.clone();
        let tool_name = call.name.clone();

        if started_now {
            self.pending.push_back(CoreOutputEvent::ToolCallStarted {
                index,
                call_id,
                name: tool_name,
            });
        }
        if !arguments.is_empty() {
            self.pending.push_back(CoreOutputEvent::ToolCallDelta {
                index,
                arguments_delta: arguments,
            });
        }
    }

    fn finish_successfully(&mut self) {
        if self.completed {
            return;
        }
        let completed: Vec<CoreOutputEvent> = self
            .tool_calls
            .iter()
            .map(|call| CoreOutputEvent::ToolCallCompleted {
                index: call.index,
                call_id: call.call_id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            })
            .collect();
        self.pending.extend(completed);

        if !matches!(self.format, ResponseFormat::Text) {
            match parse_structured(&self.text, &self.format) {
                Ok(output) => {
                    self.structured = Some(output.clone());
                    self.pending
                        .push_back(CoreOutputEvent::StructuredOutput { output });
                }
                // A schema violation is reported as an error event, but the text
                // is still delivered with a successful completion: the model
                // answered, its contract just did not match.
                Err(error) => self.record_error(&error),
            }
        }

        self.completed = true;
        self.pending.push_back(CoreOutputEvent::Completed {
            finish_reason: self.finish_reason,
            text: self.text.clone(),
            reasoning: self.reasoning.clone(),
            usage: self.usage.clone(),
            interrupted: self.interrupted,
        });
    }

    fn settle_error(&mut self, error: CoreError) {
        if self.completed {
            return;
        }
        self.record_error(&error);
        self.interrupted = true;
        self.completed = true;
        self.pending.push_back(CoreOutputEvent::Completed {
            finish_reason: self.finish_reason,
            text: self.text.clone(),
            reasoning: self.reasoning.clone(),
            usage: self.usage.clone(),
            interrupted: true,
        });
    }

    fn record_error(&mut self, error: &CoreError) {
        let output = OutputError::from_error(error);
        self.pending.push_back(CoreOutputEvent::Error {
            code: output.code.clone(),
            message: output.message.clone(),
            retryable: output.retryable,
        });
        self.error = Some(output);
    }
}

impl Stream for OutputStream {
    type Item = CoreOutputEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if let Some(event) = this.pending.pop_front() {
            return Poll::Ready(Some(event));
        }
        // A settled stream has nothing left to say: returning `Pending` here would
        // hang a consumer that keeps polling after the terminal event.
        if this.completed || this.closed {
            this.closed = true;
            return Poll::Ready(None);
        }

        if this.cancel.is_cancelled() && !this.completed {
            this.settle_error(CoreError::cancelled("output.stream"));
        } else {
            loop {
                match Pin::new(&mut this.inner).poll_next(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(None) => {
                        if !this.completed {
                            this.settle_error(CoreError::provider(
                                "output",
                                "the provider stream ended without a terminal event",
                                true,
                            ));
                        }
                        break;
                    }
                    Poll::Ready(Some(Err(error))) => {
                        this.settle_error(error);
                        break;
                    }
                    Poll::Ready(Some(Ok(event))) => {
                        this.absorb(event);
                        if let Some(event) = this.pending.pop_front() {
                            return Poll::Ready(Some(event));
                        }
                    }
                }
            }
        }

        match this.pending.pop_front() {
            Some(event) => Poll::Ready(Some(event)),
            None => {
                this.closed = true;
                Poll::Ready(None)
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;
    use crate::error::CoreResult;
    use crate::provider::config::ProviderId;
    use crate::provider::response::TransportStats;

    fn provider() -> ProviderId {
        ProviderId::new("mock").unwrap()
    }

    fn stream_with(
        events: Vec<CoreResult<StreamEvent>>,
        cancel: CancelToken,
    ) -> (OutputStream, mpsc::Sender<CoreResult<StreamEvent>>) {
        let (sender, receiver) = mpsc::channel(16);
        for event in events {
            sender.try_send(event).unwrap();
        }
        let inner = AiStream::new(provider(), "mock-1", receiver);
        (
            OutputStream::new(inner, cancel, ResponseFormat::Text),
            sender,
        )
    }

    async fn next(stream: &mut OutputStream) -> Option<CoreOutputEvent> {
        std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
    }

    #[tokio::test]
    async fn tool_call_fragments_are_assembled_in_order() {
        let (mut stream, sender) = stream_with(
            vec![
                Ok(StreamEvent::TextDelta {
                    text: "thinking".to_owned(),
                }),
                Ok(StreamEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call_1".to_owned()),
                    name: Some("lookup_word".to_owned()),
                    arguments: "{\"word\":".to_owned(),
                }),
                Ok(StreamEvent::ToolCallDelta {
                    index: 0,
                    id: None,
                    name: None,
                    arguments: "\"apple\"}".to_owned(),
                }),
                Ok(StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                    usage: None,
                    transport: TransportStats::default(),
                }),
            ],
            CancelToken::new(),
        );

        let mut events = Vec::new();
        while let Some(event) = next(&mut stream).await {
            let terminal = event.is_terminal();
            events.push(event);
            if terminal {
                break;
            }
        }
        drop(sender);

        assert!(matches!(events[0], CoreOutputEvent::Started { .. }));
        assert!(events.iter().any(|event| matches!(
            event,
            CoreOutputEvent::ToolCallStarted { call_id, .. } if call_id == "call_1"
        )));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, CoreOutputEvent::ToolCallDelta { .. }))
                .count(),
            2
        );
        let completed = events
            .iter()
            .find_map(|event| match event {
                CoreOutputEvent::ToolCallCompleted {
                    arguments, name, ..
                } => Some((name.clone(), arguments.clone())),
                _ => None,
            })
            .unwrap();
        assert_eq!(completed.0, "lookup_word");
        assert_eq!(completed.1, "{\"word\":\"apple\"}");

        let outcome = stream.into_outcome();
        assert_eq!(outcome.text, "thinking");
        assert_eq!(outcome.tool_calls.len(), 1);
        assert_eq!(outcome.finish_reason, Some(FinishReason::ToolCalls));
        assert!(!outcome.interrupted);
    }

    #[tokio::test]
    async fn cancellation_keeps_partial_text_and_emits_an_error() {
        let cancel = CancelToken::new();
        let (mut stream, _sender) = stream_with(
            vec![Ok(StreamEvent::TextDelta {
                text: "he".to_owned(),
            })],
            cancel.clone(),
        );

        assert!(matches!(
            next(&mut stream).await,
            Some(CoreOutputEvent::Started { .. })
        ));
        assert!(matches!(
            next(&mut stream).await,
            Some(CoreOutputEvent::TextDelta { .. })
        ));

        cancel.cancel();

        match next(&mut stream).await {
            Some(CoreOutputEvent::Error { code, .. }) => {
                assert_eq!(code, "CORE_CANCELLED");
            }
            other => panic!("expected an error event, got {other:?}"),
        }
        match next(&mut stream).await {
            Some(CoreOutputEvent::Completed {
                interrupted, text, ..
            }) => {
                assert!(interrupted);
                assert_eq!(text, "he");
            }
            other => panic!("expected a completion, got {other:?}"),
        }
        assert!(next(&mut stream).await.is_none());
        assert_eq!(stream.partial_text(), "he");
    }

    #[tokio::test]
    async fn provider_error_mid_stream_preserves_the_prefix() {
        let (stream, _sender) = stream_with(
            vec![
                Ok(StreamEvent::TextDelta {
                    text: "partial".to_owned(),
                }),
                Err(CoreError::provider("mock", "upstream reset", true)),
            ],
            CancelToken::new(),
        );

        let outcome = stream.drain().await;
        assert_eq!(outcome.text, "partial");
        assert!(outcome.interrupted);
        assert_eq!(
            outcome.error.as_ref().map(|error| error.code.as_str()),
            Some("CORE_PROVIDER")
        );
    }

    #[tokio::test]
    async fn absent_terminal_event_is_reported_as_interrupted() {
        let (stream, sender) = stream_with(
            vec![Ok(StreamEvent::TextDelta {
                text: "x".to_owned(),
            })],
            CancelToken::new(),
        );
        drop(sender);
        let outcome = stream.drain().await;
        assert!(outcome.interrupted);
        assert_eq!(outcome.text, "x");
        assert_eq!(
            outcome.error.as_ref().map(|error| error.code.as_str()),
            Some("CORE_PROVIDER")
        );
    }

    #[tokio::test]
    async fn started_event_carries_correlation_when_available() {
        let (mut stream, _sender) = stream_with(vec![], CancelToken::new());
        let first = next(&mut stream).await.unwrap();
        match first {
            CoreOutputEvent::Started {
                provider_id,
                model_id,
                request_id,
                ..
            } => {
                assert_eq!(provider_id.as_str(), "mock");
                assert_eq!(model_id, "mock-1");
                assert!(request_id.is_none());
            }
            other => panic!("expected Started, got {other:?}"),
        }
    }
}
