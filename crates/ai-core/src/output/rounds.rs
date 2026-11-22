//! Multi-round tool loop: model → tool execution → model, until settled.
//!
//! # Responsibility
//!
//! Own the one thing an output engine deliberately does not: replay. A model
//! answer that requests tool calls is not a final answer, so the loop
//!
//! 1. asks the model for a round,
//! 2. turns every requested call into a [`ToolExecution`] (through
//!    [`ToolExecutor`]), emitting `ToolCallExecuted` as each one settles,
//! 3. appends the assistant turn and one `tool_result` message per call,
//! 4. repeats with the same `AiRequest` value plus the replayed messages.
//!
//! # Contracts
//!
//! * A round is *settled* when it requests no tool call
//!   ([`RoundStopReason::Completed`]) or when the model answer was interrupted
//!   ([`RoundStopReason::Interrupted`], which still carries the partial text).
//! * A tool failure is data, not an abort: the failed result is replayed so the
//!   model can recover, exactly like a successful one.
//! * The round budget is hard: once it is spent the loop stops with
//!   [`RoundStopReason::ToolRoundsExhausted`] and returns the last answer instead
//!   of looping forever.
//! * Only `ToolCallExecuted` events are produced here; the caller still owns
//!   `Started`/`Completed` semantics for its own stream. Streaming callers drive
//!   [`OutputStream`](crate::output::OutputStream) directly.

use std::sync::Arc;

use crate::config::{ToolConfig, DEFAULT_TOOL_MAX_ROUNDS, MAX_TOOL_ROUNDS};
use crate::error::{CoreError, CoreResult};
use crate::output::engine::OutputEngine;
use crate::output::event::{CoreOutputEvent, OutputOutcome, OutputToolCall};
use crate::provider::response::ToolCall;
use crate::provider::router::CallContext;
use crate::provider::types::{AiRequest, Message};
use crate::tool::{ToolExecution, ToolExecutor};

/// Why the loop stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundStopReason {
    /// The model answered without requesting more tools.
    Completed,
    /// The model answer was interrupted by an error, timeout or cancellation.
    Interrupted,
    /// The round budget was spent while the model still requested tools.
    ToolRoundsExhausted,
}

impl RoundStopReason {
    /// Stable label, safe to put in an event payload.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Interrupted => "interrupted",
            Self::ToolRoundsExhausted => "tool_rounds_exhausted",
        }
    }
}

impl std::fmt::Display for RoundStopReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One model round and the tool executions it triggered.
#[derive(Debug, Clone, PartialEq)]
pub struct Round {
    /// Normalized answer of this round.
    pub outcome: OutputOutcome,
    /// Executions produced by the calls of this round (empty when it settled).
    pub executions: Vec<ToolExecution>,
}

impl Round {
    /// Whether this round requested at least one tool call.
    #[must_use]
    pub fn requested_tools(&self) -> bool {
        !self.outcome.tool_calls.is_empty()
    }
}

/// Result of driving a tool loop to its end.
#[derive(Debug, Clone, PartialEq)]
pub struct RoundOutcome {
    /// Every round, oldest first.
    pub rounds: Vec<Round>,
    /// Every execution across all rounds, in call order.
    pub executions: Vec<ToolExecution>,
    /// Answer of the last round (the one reported to the user).
    pub final_outcome: OutputOutcome,
    /// Why the loop stopped.
    pub stop_reason: RoundStopReason,
    /// Assistant text of every round, concatenated in order.
    pub text: String,
}

impl RoundOutcome {
    /// Number of model rounds performed.
    #[must_use]
    pub fn round_count(&self) -> usize {
        self.rounds.len()
    }

    /// Whether the last round settled without an interruption.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.stop_reason == RoundStopReason::Completed && !self.final_outcome.interrupted
    }

    /// Whether the loop stopped because the round budget was spent.
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.stop_reason == RoundStopReason::ToolRoundsExhausted
    }

    /// Executions that failed, in call order.
    pub fn failed_executions(&self) -> impl Iterator<Item = &ToolExecution> {
        self.executions
            .iter()
            .filter(|execution| !execution.success)
    }
}

/// Drives provider rounds until the model stops asking for tools.
#[derive(Debug, Clone)]
pub struct RoundLoop {
    engine: Arc<OutputEngine>,
    executor: ToolExecutor,
    max_rounds: u32,
}

impl RoundLoop {
    /// Builds a loop with the default round budget.
    #[must_use]
    pub fn new(engine: Arc<OutputEngine>, executor: ToolExecutor) -> Self {
        Self {
            engine,
            executor,
            max_rounds: DEFAULT_TOOL_MAX_ROUNDS,
        }
    }

    /// Builds a loop from the `tools` configuration section.
    #[must_use]
    pub fn from_config(
        engine: Arc<OutputEngine>,
        executor: ToolExecutor,
        config: &ToolConfig,
    ) -> Self {
        Self {
            engine,
            executor,
            max_rounds: config.max_rounds,
        }
    }

    /// Sets the round budget.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when `rounds` is zero or above
    /// [`MAX_TOOL_ROUNDS`].
    pub fn with_max_rounds(mut self, rounds: u32) -> CoreResult<Self> {
        if rounds == 0 || rounds > MAX_TOOL_ROUNDS {
            return Err(CoreError::invalid_argument(
                "tools.max_rounds",
                format!("must be between 1 and {MAX_TOOL_ROUNDS}"),
            ));
        }
        self.max_rounds = rounds;
        Ok(self)
    }

    /// Configured round budget.
    #[must_use]
    pub const fn max_rounds(&self) -> u32 {
        self.max_rounds
    }

    /// The executor used for every call.
    #[must_use]
    pub fn executor(&self) -> &ToolExecutor {
        &self.executor
    }

    /// Runs the loop, emitting one `ToolCallExecuted` per execution into `sink`.
    ///
    /// # Errors
    ///
    /// Propagates provider/routing errors unchanged. Tool failures are *not*
    /// errors: they are replayed to the model and reported in
    /// [`RoundOutcome::executions`].
    pub async fn run<F>(
        &self,
        mut request: AiRequest,
        context: &CallContext,
        mut sink: F,
    ) -> CoreResult<RoundOutcome>
    where
        F: FnMut(&CoreOutputEvent),
    {
        let mut rounds = Vec::new();
        let mut executions = Vec::new();
        let mut text = String::new();
        let mut stop_reason = RoundStopReason::ToolRoundsExhausted;

        for _ in 0..self.max_rounds {
            let outcome = self.engine.complete(request.clone(), context).await?;
            text.push_str(&outcome.text);

            if outcome.interrupted {
                stop_reason = RoundStopReason::Interrupted;
                rounds.push(Round {
                    outcome,
                    executions: Vec::new(),
                });
                break;
            }
            if !outcome.has_tool_calls() {
                stop_reason = RoundStopReason::Completed;
                rounds.push(Round {
                    outcome,
                    executions: Vec::new(),
                });
                break;
            }

            let executed = self.replay_round(&mut request, &outcome, &mut sink).await;
            executions.extend(executed.iter().cloned());
            rounds.push(Round {
                outcome,
                executions: executed,
            });
        }

        let final_outcome = match rounds.last() {
            Some(round) => round.outcome.clone(),
            // Unreachable: `max_rounds` is validated to be at least one, so the
            // loop always records a round. A default keeps the fallback total.
            None => empty_outcome(),
        };

        Ok(RoundOutcome {
            rounds,
            executions,
            final_outcome,
            stop_reason,
            text,
        })
    }

    /// Runs the same loop over streaming rounds.
    ///
    /// Every event of every round is forwarded to `sink` in order, so the host
    /// renders deltas exactly as it would for a single stream. Because each model
    /// round is its own stream, a two-round answer produces two `Started` and two
    /// `Completed` events; `ToolCallExecuted` sits between them. Use
    /// [`RoundOutcome::stop_reason`] — not the last `Completed` — to tell whether
    /// the logical answer settled.
    ///
    /// # Errors
    ///
    /// Propagates routing errors raised before the first delta;
    /// errors that happen mid-stream arrive as an `Error` event and end the
    /// round with `interrupted = true`.
    pub async fn run_streaming<F>(
        &self,
        mut request: AiRequest,
        context: &CallContext,
        mut sink: F,
    ) -> CoreResult<RoundOutcome>
    where
        F: FnMut(&CoreOutputEvent),
    {
        let mut rounds = Vec::new();
        let mut executions = Vec::new();
        let mut text = String::new();
        let mut stop_reason = RoundStopReason::ToolRoundsExhausted;

        for _ in 0..self.max_rounds {
            let stream = self.engine.stream(request.clone(), context).await?;
            let outcome = stream.run(|event| sink(event)).await;
            text.push_str(&outcome.text);

            if outcome.interrupted {
                stop_reason = RoundStopReason::Interrupted;
                rounds.push(Round {
                    outcome,
                    executions: Vec::new(),
                });
                break;
            }
            if !outcome.has_tool_calls() {
                stop_reason = RoundStopReason::Completed;
                rounds.push(Round {
                    outcome,
                    executions: Vec::new(),
                });
                break;
            }

            let executed = self.replay_round(&mut request, &outcome, &mut sink).await;
            executions.extend(executed.iter().cloned());
            rounds.push(Round {
                outcome,
                executions: executed,
            });
        }

        let final_outcome = match rounds.last() {
            Some(round) => round.outcome.clone(),
            None => empty_outcome(),
        };

        Ok(RoundOutcome {
            rounds,
            executions,
            final_outcome,
            stop_reason,
            text,
        })
    }

    /// Replays one tool-calling round and returns its executions.
    async fn replay_round<F>(
        &self,
        request: &mut AiRequest,
        outcome: &OutputOutcome,
        sink: &mut F,
    ) -> Vec<ToolExecution>
    where
        F: FnMut(&CoreOutputEvent),
    {
        let calls = outcome.tool_calls.clone();
        request.push_message(Message::assistant_tool_calls(
            outcome.text.clone(),
            calls.iter().map(tool_call_of).collect(),
        ));
        let executed = self.executor.execute_all(&calls, sink).await;
        for execution in &executed {
            request.push_message(Message::tool_result(
                execution.call_id.clone(),
                execution.output.clone(),
            ));
        }
        executed
    }
}

/// Converts a normalized call back into the provider-side type used to replay it.
fn tool_call_of(call: &OutputToolCall) -> ToolCall {
    ToolCall {
        id: call.call_id.clone(),
        name: call.name.clone(),
        arguments: call.arguments.clone(),
    }
}

/// Empty answer used only if the loop body never ran.
fn empty_outcome() -> OutputOutcome {
    OutputOutcome {
        text: String::new(),
        reasoning: String::new(),
        structured: None,
        tool_calls: Vec::new(),
        usage: None,
        finish_reason: None,
        interrupted: false,
        error: None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::time::Duration;

    use serde_json::{json, Value};

    use super::*;
    use crate::config::ConfigStore;
    use crate::provider::config::{ModelConfig, ProviderConfig, ProviderId, ProviderProtocol};
    use crate::provider::response::ToolCall;
    use crate::provider::types::ToolSpec;
    use crate::provider::{AdapterFactory, MockAdapter, MockFailure, ProviderRuntime};
    use crate::tool::{Tool, ToolFuture, ToolRegistry};
    use crate::AiCoreConfig;

    struct EchoTool {
        spec: ToolSpec,
    }

    impl EchoTool {
        fn new() -> Self {
            Self {
                spec: ToolSpec::new(
                    "echo",
                    "echoes its arguments",
                    json!({"type": "object", "properties": {}}),
                )
                .unwrap(),
            }
        }
    }

    impl Tool for EchoTool {
        fn name(&self) -> &str {
            self.spec.name()
        }

        fn description(&self) -> &str {
            self.spec.description()
        }

        fn parameters(&self) -> Value {
            self.spec.parameters().clone()
        }

        fn invoke<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
            Box::pin(async move { Ok(arguments.clone()) })
        }
    }

    fn provider_config() -> ProviderConfig {
        ProviderConfig::new(
            ProviderId::new("mock").unwrap(),
            ProviderProtocol::OpenAiCompatible,
        )
        .with_api_key("test-key")
        .with_model(ModelConfig::new("mock-1"))
    }

    fn engine_for(mock: &MockAdapter) -> Arc<OutputEngine> {
        let mut config = AiCoreConfig::default();
        config
            .providers
            .insert(provider_config().id.clone(), provider_config());
        let store = Arc::new(ConfigStore::new(config).unwrap());
        let factory = AdapterFactory::new().unwrap();
        factory.bind(mock.provider_id().clone(), Arc::new(mock.clone()));
        let runtime = ProviderRuntime::with_factory(store, factory).unwrap();
        Arc::new(OutputEngine::new(Arc::new(runtime)))
    }

    fn executor() -> ToolExecutor {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool::new())).unwrap();
        ToolExecutor::new(Arc::new(registry), Duration::from_secs(1))
    }

    fn mock() -> MockAdapter {
        MockAdapter::new(ProviderId::new("mock").unwrap())
    }

    fn call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: "{}".to_owned(),
        }
    }

    #[tokio::test]
    async fn replays_a_tool_round_and_settles() {
        let mock = mock()
            .respond_tool_calls("", vec![call("call_1", "echo")])
            .respond_text("done");
        let engine = engine_for(&mock);
        let loop_runner = RoundLoop::new(engine, executor());
        let command = CallContext::generate();

        let mut events = Vec::new();
        let outcome = loop_runner
            .run(AiRequest::user_prompt("hi"), &command, |event| {
                events.push(event.clone())
            })
            .await
            .unwrap();

        assert_eq!(outcome.round_count(), 2);
        assert!(outcome.is_complete());
        assert_eq!(outcome.stop_reason, RoundStopReason::Completed);
        assert_eq!(outcome.text, "done");
        assert_eq!(outcome.executions.len(), 1);
        assert!(outcome.executions[0].is_success());
        assert!(outcome.failed_executions().next().is_none());
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            CoreOutputEvent::ToolCallExecuted { success: true, .. }
        ));

        // The second provider call must carry the replayed exchange.
        let calls = mock.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls[1].message_count >= 3);
    }

    #[tokio::test]
    async fn streaming_rounds_forward_every_event_in_order() {
        let mock = mock()
            .respond_tool_calls("", vec![call("call_1", "echo")])
            .respond_text("done");
        let engine = engine_for(&mock);
        let loop_runner = RoundLoop::new(engine, executor());

        let mut events = Vec::new();
        let outcome = loop_runner
            .run_streaming(
                AiRequest::user_prompt("hi"),
                &CallContext::generate(),
                |event| events.push(event.clone()),
            )
            .await
            .unwrap();

        assert!(outcome.is_complete());
        assert_eq!(outcome.round_count(), 2);
        assert_eq!(outcome.text, "done");
        assert_eq!(outcome.executions.len(), 1);

        let count =
            |wanted: fn(&CoreOutputEvent) -> bool| events.iter().filter(|e| wanted(e)).count();
        assert_eq!(count(|e| matches!(e, CoreOutputEvent::Started { .. })), 2);
        assert_eq!(count(|e| matches!(e, CoreOutputEvent::Completed { .. })), 2);
        assert_eq!(
            count(|e| matches!(e, CoreOutputEvent::ToolCallDelta { .. })),
            1
        );
        assert_eq!(
            count(|e| matches!(e, CoreOutputEvent::ToolCallExecuted { .. })),
            1
        );

        // The execution event must sit between the two terminal round events.
        let completed_index = events
            .iter()
            .position(|event| matches!(event, CoreOutputEvent::Completed { .. }))
            .unwrap();
        let executed_index = events
            .iter()
            .position(|event| matches!(event, CoreOutputEvent::ToolCallExecuted { .. }))
            .unwrap();
        assert!(completed_index < executed_index);
    }

    #[tokio::test]
    async fn a_plain_answer_settles_in_one_round() {
        let mock = mock().respond_text("hello");
        let engine = engine_for(&mock);
        let loop_runner = RoundLoop::new(engine, executor());

        let outcome = loop_runner
            .run(
                AiRequest::user_prompt("hi"),
                &CallContext::generate(),
                |_| {},
            )
            .await
            .unwrap();

        assert_eq!(outcome.round_count(), 1);
        assert!(outcome.is_complete());
        assert!(outcome.executions.is_empty());
        assert_eq!(outcome.final_outcome.text, "hello");
    }

    #[tokio::test]
    async fn the_round_budget_is_hard() {
        let mock = mock()
            .respond_tool_calls("thinking", vec![call("call_1", "echo")])
            .respond_tool_calls("still", vec![call("call_2", "echo")]);
        let engine = engine_for(&mock);
        let loop_runner = RoundLoop::new(engine, executor())
            .with_max_rounds(2)
            .unwrap();

        let outcome = loop_runner
            .run(
                AiRequest::user_prompt("hi"),
                &CallContext::generate(),
                |_| {},
            )
            .await
            .unwrap();

        assert_eq!(outcome.round_count(), 2);
        assert!(outcome.is_exhausted());
        assert!(!outcome.is_complete());
        assert_eq!(outcome.executions.len(), 2);
        assert_eq!(outcome.final_outcome.text, "still");
        assert_eq!(outcome.text, "thinkingstill");
    }

    #[tokio::test]
    async fn a_failed_tool_is_replayed_not_fatal() {
        let mock = mock()
            .respond_tool_calls("", vec![call("call_1", "missing")])
            .respond_text("recovered");
        let engine = engine_for(&mock);
        let loop_runner = RoundLoop::new(engine, executor());

        let outcome = loop_runner
            .run(
                AiRequest::user_prompt("hi"),
                &CallContext::generate(),
                |_| {},
            )
            .await
            .unwrap();

        assert!(outcome.is_complete());
        assert_eq!(outcome.executions.len(), 1);
        assert!(!outcome.executions[0].is_success());
        assert!(outcome.executions[0].output.contains("CORE_TOOL"));
        assert_eq!(outcome.failed_executions().count(), 1);
        assert_eq!(outcome.text, "recovered");
    }

    #[tokio::test]
    async fn provider_errors_propagate() {
        let mock = mock().fail(MockFailure::NonRetryable);
        let engine = engine_for(&mock);
        let loop_runner = RoundLoop::new(engine, executor());

        let error = loop_runner
            .run(
                AiRequest::user_prompt("hi"),
                &CallContext::generate(),
                |_| {},
            )
            .await
            .unwrap_err();
        assert_eq!(error.error_code(), "CORE_PROVIDER");
    }

    #[test]
    fn round_budget_is_validated() {
        let mock = mock();
        let loop_runner = RoundLoop::new(engine_for(&mock), executor());
        assert_eq!(loop_runner.max_rounds(), DEFAULT_TOOL_MAX_ROUNDS);
        assert!(loop_runner.clone().with_max_rounds(0).is_err());
        assert!(loop_runner
            .clone()
            .with_max_rounds(MAX_TOOL_ROUNDS + 1)
            .is_err());
        assert_eq!(loop_runner.with_max_rounds(2).unwrap().max_rounds(), 2);
    }

    #[test]
    fn stop_reasons_have_stable_labels() {
        assert_eq!(RoundStopReason::Completed.as_str(), "completed");
        assert_eq!(RoundStopReason::Interrupted.as_str(), "interrupted");
        assert_eq!(
            RoundStopReason::ToolRoundsExhausted.as_str(),
            "tool_rounds_exhausted"
        );
        assert_eq!(RoundStopReason::Completed.to_string(), "completed");
    }

    #[test]
    fn rounds_expose_whether_tools_were_requested() {
        let round = Round {
            outcome: empty_outcome(),
            executions: Vec::new(),
        };
        assert!(!round.requested_tools());
    }
}
