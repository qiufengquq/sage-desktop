//! Tool/function-calling boundary: declarations, registry, validation and execution.
//!
//! # Responsibility
//!
//! Describe callable tools to the model ([`ToolSpec`]), keep the executable
//! implementations in a [`ToolRegistry`], validate the arguments the model
//! produced and run the matching tool under the framework's timeout and
//! cancellation rules.
//!
//! # Execution rules
//!
//! * Tool execution is untrusted work: [`ToolExecutor`] runs every call through
//!   [`run_with_limits`], so a call is bounded by a deadline and aborts when the
//!   caller's [`CancelToken`] fires. A blocking or CPU-heavy implementation must
//!   dispatch through [`run_blocking`] on its own side.
//! * A tool failure is a *data* outcome, never a process failure: it is reported
//!   as a [`ToolExecution`] with `success = false` and a redacted message, so the
//!   loop can feed it back to the model instead of aborting the call.
//! * Arguments that are not valid JSON, or that violate the declared schema, are
//!   rejected with [`CoreError::InvalidArgument`]; an unknown tool name is
//!   [`CoreError::Tool`].
//! * The registry receives only the parsed arguments: a tool cannot reach
//!   provider credentials or the request envelope.
//!
//! # Event flow
//!
//! [`ToolExecutor::execute_all`] converts every result into a
//! [`CoreOutputEvent::ToolCallExecuted`] before handing it to the sink, so the
//! host sees `ToolCallCompleted` (model side) followed by `ToolCallExecuted`
//! (framework side) in the same stream.
//!
//! [`run_blocking`]: crate::async_utils::run_blocking

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::async_utils::{run_with_limits, CancelToken};
use crate::error::{CoreError, CoreResult};
use crate::output::event::{CoreOutputEvent, OutputToolCall};
use crate::provider::types::ToolSpec;
use crate::secret::sanitize_log_text;

/// Operation label used by timeout/cancellation errors raised while a tool runs.
const TOOL_OPERATION: &str = "tool.execute";

/// Boxed, `Send` future returned by [`Tool::invoke`].
///
/// Boxing keeps [`Tool`] object-safe: the registry stores `Arc<dyn Tool>`, and
/// async methods returning `impl Future` would require `async fn` in traits
/// (unstable at the crate's MSRV).
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = CoreResult<Value>> + Send + 'a>>;

/// A callable tool exposed to the model.
///
/// Implementations must be cheap to share (`Arc<dyn Tool>`), stateless with
/// respect to one call, and must not retain the arguments after `invoke`
/// returns.
pub trait Tool: Send + Sync {
    /// Unique name the model uses to call the tool.
    fn name(&self) -> &str;

    /// Short description shown to the model.
    fn description(&self) -> &str;

    /// JSON schema of the arguments; must be an object schema.
    fn parameters(&self) -> Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }

    /// Executes the tool with parsed, schema-validated arguments.
    ///
    /// # Errors
    ///
    /// Any [`CoreError`] is turned into a failed [`ToolExecution`] by the
    /// executor; it never aborts the surrounding call.
    fn invoke<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a>;
}

/// A tool plus its validated declaration.
#[derive(Clone)]
struct RegisteredTool {
    spec: ToolSpec,
    tool: Arc<dyn Tool>,
}

/// Registry of the tools a host exposes to the model.
///
/// Names are unique and ordered (`BTreeMap`), so [`ToolRegistry::definitions`]
/// produces a stable declaration list for the provider request.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, RegisteredTool>,
}

impl fmt::Debug for ToolRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolRegistry")
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ToolRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a tool under its own name.
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidArgument`] when the name is empty, the argument
    /// schema is not a JSON object, or the name is already taken.
    pub fn register(&mut self, tool: Arc<dyn Tool>) -> CoreResult<()> {
        let spec = ToolSpec::new(
            tool.name().to_owned(),
            tool.description().to_owned(),
            tool.parameters(),
        )?;
        if self.tools.contains_key(spec.name()) {
            return Err(CoreError::invalid_argument(
                "tool.name",
                format!("tool `{}` is already registered", spec.name()),
            ));
        }
        self.tools
            .insert(spec.name().to_owned(), RegisteredTool { spec, tool });
        Ok(())
    }

    /// Returns the executable tool registered under `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name).map(|entry| &entry.tool)
    }

    /// Whether a tool is registered under `name`.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Declarations to attach to a provider request, in stable name order.
    #[must_use]
    pub fn definitions(&self) -> Vec<ToolSpec> {
        self.tools
            .values()
            .map(|entry| entry.spec.clone())
            .collect()
    }

    /// Registered names, in stable order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tools.keys().map(String::as_str)
    }

    /// Number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether no tool is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    fn entry(&self, name: &str) -> Option<&RegisteredTool> {
        self.tools.get(name)
    }
}

/// Outcome of executing one model-requested tool call.
///
/// `output` is the serialized tool result on success, or a redacted,
/// code-prefixed message on failure — never raw arguments and never credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolExecution {
    /// Position of the call in the model response.
    pub index: u16,
    /// Provider-assigned call id, needed to submit the result back.
    pub call_id: String,
    /// Tool name.
    pub name: String,
    /// Whether the tool returned successfully.
    pub success: bool,
    /// Serialized result, or a redacted structured error message.
    pub output: String,
    /// Wall-clock execution time in milliseconds.
    pub duration_ms: u64,
}

impl ToolExecution {
    /// Whether the tool completed successfully.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.success
    }

    /// The output event describing this execution.
    #[must_use]
    pub fn event(&self) -> CoreOutputEvent {
        CoreOutputEvent::ToolCallExecuted {
            index: self.index,
            call_id: self.call_id.clone(),
            name: self.name.clone(),
            success: self.success,
            output: self.output.clone(),
            duration_ms: self.duration_ms,
        }
    }
}

/// Executes model-requested tool calls against a [`ToolRegistry`].
///
/// The executor is cheap to clone and shares the registry; the deadline and the
/// cancellation token apply to each individual call.
#[derive(Debug, Clone)]
pub struct ToolExecutor {
    registry: Arc<ToolRegistry>,
    limit: Duration,
    cancel: CancelToken,
}

impl ToolExecutor {
    /// Builds an executor with a per-call deadline.
    #[must_use]
    pub fn new(registry: Arc<ToolRegistry>, limit: Duration) -> Self {
        Self {
            registry,
            limit,
            cancel: CancelToken::new(),
        }
    }

    /// Builds an executor whose per-call deadline comes from configuration.
    #[must_use]
    pub fn from_config(registry: Arc<ToolRegistry>, config: &crate::config::ToolConfig) -> Self {
        Self::new(registry, config.call_timeout)
    }

    /// Attaches the caller's cancellation token.
    #[must_use]
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// The registry consulted by this executor.
    #[must_use]
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// Executes one call, converting any failure into a failed execution.
    ///
    /// This never returns an error: an unknown tool, invalid arguments, a
    /// timeout or a cancellation all become `success = false` so the result can
    /// be replayed to the model.
    pub async fn execute(&self, call: &OutputToolCall) -> ToolExecution {
        let started = Instant::now();
        let outcome = self.invoke(call).await;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

        let (success, output) = match outcome {
            Ok(value) => match serde_json::to_string(&value) {
                Ok(serialized) => (true, serialized),
                Err(error) => (
                    false,
                    failure_text(&CoreError::serialization_with_source(
                        "failed to encode the tool result",
                        error,
                    )),
                ),
            },
            Err(error) => (false, failure_text(&error)),
        };

        // Arguments are never logged: only the identity and the timing.
        tracing::debug!(
            tool = %call.name,
            call_id = %call.call_id,
            success,
            duration_ms,
            "tool call executed"
        );

        ToolExecution {
            index: call.index,
            call_id: call.call_id.clone(),
            name: call.name.clone(),
            success,
            output,
            duration_ms,
        }
    }

    /// Executes every call in order, emitting a
    /// [`CoreOutputEvent::ToolCallExecuted`] to `sink` as each one settles.
    ///
    /// Sequential execution is deliberate: model-emitted calls commonly depend
    /// on each other, and a host that needs parallelism can call
    /// [`ToolExecutor::execute`] itself.
    pub async fn execute_all<F>(&self, calls: &[OutputToolCall], mut sink: F) -> Vec<ToolExecution>
    where
        F: FnMut(&CoreOutputEvent),
    {
        let mut executions = Vec::with_capacity(calls.len());
        for call in calls {
            let execution = self.execute(call).await;
            sink(&execution.event());
            executions.push(execution);
        }
        executions
    }

    async fn invoke(&self, call: &OutputToolCall) -> CoreResult<Value> {
        let entry = self
            .registry
            .entry(&call.name)
            .ok_or_else(|| CoreError::tool(&call.name, "is not registered"))?;
        let arguments = parse_arguments(&call.arguments)?;
        validate_arguments(&entry.spec, &arguments)?;
        run_with_limits(
            TOOL_OPERATION,
            self.limit,
            &self.cancel,
            entry.tool.invoke(&arguments),
        )
        .await?
    }
}

/// Parses the model-produced argument string, treating empty input as `{}`.
fn parse_arguments(raw: &str) -> CoreResult<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    serde_json::from_str(trimmed)
        .map_err(|_| CoreError::invalid_argument("tool.arguments", "must be valid JSON"))
}

/// Enforces the schema subset the registry can check without a full validator:
/// the declared root type and the `required` property list.
fn validate_arguments(spec: &ToolSpec, arguments: &Value) -> CoreResult<()> {
    let schema = spec.parameters();
    if schema.get("type").and_then(Value::as_str) == Some("object") && !arguments.is_object() {
        return Err(CoreError::invalid_argument(
            "tool.arguments",
            "must be a JSON object",
        ));
    }

    let Some(required) = schema.get("required").and_then(Value::as_array) else {
        return Ok(());
    };
    for name in required.iter().filter_map(Value::as_str) {
        if arguments.get(name).is_none() {
            return Err(CoreError::invalid_argument(
                format!("tool.arguments.{name}"),
                "is required",
            ));
        }
    }
    Ok(())
}

/// Redacted, code-prefixed failure text safe to hand back to the model.
fn failure_text(error: &CoreError) -> String {
    sanitize_log_text(&format!("{}: {error}", error.error_code()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    struct EchoTool {
        spec: ToolSpec,
    }

    impl EchoTool {
        fn new() -> Self {
            let spec = ToolSpec::new(
                "echo",
                "echoes its arguments",
                json!({
                    "type": "object",
                    "required": ["word"],
                    "properties": { "word": { "type": "string" } }
                }),
            )
            .unwrap();
            Self { spec }
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

    struct SleepTool {
        spec: ToolSpec,
    }

    impl SleepTool {
        fn new() -> Self {
            let spec = ToolSpec::new(
                "sleep",
                "never returns in time",
                json!({ "type": "object" }),
            )
            .unwrap();
            Self { spec }
        }
    }

    impl Tool for SleepTool {
        fn name(&self) -> &str {
            self.spec.name()
        }

        fn description(&self) -> &str {
            self.spec.description()
        }

        fn parameters(&self) -> Value {
            self.spec.parameters().clone()
        }

        fn invoke<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(Value::Null)
            })
        }
    }

    fn call(name: &str, arguments: &str) -> OutputToolCall {
        OutputToolCall {
            index: 0,
            call_id: "call_1".to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        }
    }

    fn registry_with(tools: Vec<Arc<dyn Tool>>) -> Arc<ToolRegistry> {
        let mut registry = ToolRegistry::new();
        for tool in tools {
            registry.register(tool).unwrap();
        }
        Arc::new(registry)
    }

    #[test]
    fn configuration_supplies_the_call_deadline() {
        let config = crate::config::ToolConfig {
            max_rounds: 2,
            call_timeout: Duration::from_millis(250),
        };
        let executor = ToolExecutor::from_config(registry_with(vec![]), &config);
        assert!(executor.registry().is_empty());
    }

    #[test]
    fn registry_rejects_duplicate_names() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool::new())).unwrap();
        let error = registry.register(Arc::new(EchoTool::new())).unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::InvalidArgument);
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.names().collect::<Vec<_>>(), vec!["echo"]);
    }

    #[test]
    fn definitions_are_stable_and_ordered() {
        let registry = registry_with(vec![Arc::new(EchoTool::new()), Arc::new(SleepTool::new())]);
        let names: Vec<String> = registry
            .definitions()
            .iter()
            .map(|spec| spec.name().to_owned())
            .collect();
        assert_eq!(names, vec!["echo".to_owned(), "sleep".to_owned()]);
        assert!(registry.contains("echo"));
        assert!(!registry.contains("missing"));
    }

    #[tokio::test]
    async fn executes_a_registered_tool_and_emits_an_event() {
        let executor = ToolExecutor::new(
            registry_with(vec![Arc::new(EchoTool::new())]),
            Duration::from_secs(1),
        );
        let execution = executor.execute(&call("echo", r#"{"word":"apple"}"#)).await;

        assert!(execution.is_success());
        assert_eq!(execution.output, r#"{"word":"apple"}"#);
        assert_eq!(execution.call_id, "call_1");

        match execution.event() {
            CoreOutputEvent::ToolCallExecuted {
                name,
                success,
                output,
                duration_ms,
                ..
            } => {
                assert_eq!(name, "echo");
                assert!(success);
                assert_eq!(output, r#"{"word":"apple"}"#);
                assert_eq!(duration_ms, execution.duration_ms);
            }
            other => panic!("expected ToolCallExecuted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_tool_fails_without_leaking_arguments() {
        let executor = ToolExecutor::new(registry_with(vec![]), Duration::from_secs(1));
        let execution = executor
            .execute(&call("missing", r#"{"api_key":"sk-secret12345678"}"#))
            .await;

        assert!(!execution.is_success());
        assert!(execution.output.contains("CORE_TOOL"));
        assert!(!execution.output.contains("sk-secret12345678"));
    }

    #[tokio::test]
    async fn invalid_arguments_are_rejected_before_invocation() {
        let executor = ToolExecutor::new(
            registry_with(vec![Arc::new(EchoTool::new())]),
            Duration::from_secs(1),
        );

        let malformed = executor.execute(&call("echo", "not json")).await;
        assert!(!malformed.is_success());
        assert!(malformed.output.contains("CORE_INVALID_ARGUMENT"));

        let incomplete = executor.execute(&call("echo", r#"{}"#)).await;
        assert!(!incomplete.is_success());
        assert!(incomplete.output.contains("CORE_INVALID_ARGUMENT"));
    }

    #[tokio::test]
    async fn empty_arguments_are_treated_as_an_empty_object() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(SleepTool::new())).unwrap();
        let executor = ToolExecutor::new(Arc::new(registry), Duration::from_millis(10));

        let execution = executor.execute(&call("sleep", "   ")).await;
        // The tool was reached and timed out instead of failing validation.
        assert!(!execution.is_success());
        assert!(execution.output.contains("CORE_TIMEOUT"));
    }

    #[tokio::test]
    async fn deadline_bounds_a_slow_tool() {
        let executor = ToolExecutor::new(
            registry_with(vec![Arc::new(SleepTool::new())]),
            Duration::from_millis(10),
        );
        let execution = executor.execute(&call("sleep", "{}")).await;
        assert!(!execution.is_success());
        assert!(execution.output.contains("CORE_TIMEOUT"));
    }

    #[tokio::test]
    async fn cancellation_bounds_a_running_tool() {
        let cancel = CancelToken::new();
        cancel.cancel();
        let executor = ToolExecutor::new(
            registry_with(vec![Arc::new(SleepTool::new())]),
            Duration::from_secs(30),
        )
        .with_cancel(cancel);

        let execution = executor.execute(&call("sleep", "{}")).await;
        assert!(!execution.is_success());
        assert!(execution.output.contains("CORE_CANCELLED"));
    }

    #[tokio::test]
    async fn execute_all_emits_one_event_per_call_in_order() {
        let executor = ToolExecutor::new(
            registry_with(vec![Arc::new(EchoTool::new())]),
            Duration::from_secs(1),
        );
        let calls = vec![call("echo", r#"{"word":"one"}"#), call("missing", "{}")];

        let mut events = Vec::new();
        let executions = executor
            .execute_all(&calls, |event| events.push(event.clone()))
            .await;

        assert_eq!(executions.len(), 2);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            CoreOutputEvent::ToolCallExecuted { success: true, .. }
        ));
        assert!(matches!(
            &events[1],
            CoreOutputEvent::ToolCallExecuted { success: false, .. }
        ));
    }
}
