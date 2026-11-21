//! Context assembly: instructions + replayed turns + memory → a token budget.
//!
//! # Responsibility
//!
//! Turn stored/replayed messages into the exact message list sent to a provider:
//! pin the system instructions, keep the newest turns that fit the budget, drop
//! the rest, and never hand a provider a broken tool exchange.
//!
//! # Determinism
//!
//! Assembly depends only on its inputs (the message list, the policy and the
//! estimator), never on wall-clock time or randomness. Two calls with the same
//! inputs produce byte-identical output, which is what makes a compacted
//! conversation reproducible and testable. Compaction is *count-based*: the
//! dropped turns are replaced by one omission marker, never by a model summary
//! (that would require another provider round and is a host decision).
//!
//! # Budget rules
//!
//! * The input budget is `max_input_tokens - reserve_output_tokens`.
//! * System messages are pinned: they are always sent, even when they alone
//!   exceed the budget (the result then reports `over_budget = true` instead of
//!   silently dropping the instructions).
//! * Non-system messages are admitted newest-first and replayed oldest-first.
//! * A tool exchange is atomic: an assistant turn that calls tools is only kept
//!   when **all** of its results are kept, and orphaned results (whose call was
//!   dropped) are never sent — both OpenAI and Anthropic reject that shape.
//! * Token counts are estimates, produced by a [`TokenEstimator`]; the default
//!   [`HeuristicEstimator`] is the documented `chars / 4` approximation, not a
//!   real tokenizer.

use std::sync::Arc;

use crate::config::ContextConfig;
use crate::error::{CoreError, CoreResult};
use crate::ids::SessionId;
use crate::memory::{MemoryMessage, MemoryRole, MemoryStore};
use crate::provider::types::{ContentPart, Message, Role};

/// Approximate characters per token used by [`HeuristicEstimator`].
pub const DEFAULT_CHARS_PER_TOKEN: usize = 4;
/// Tokens charged for every message, covering role and separator markers.
pub const MESSAGE_OVERHEAD_TOKENS: usize = 4;
/// Tokens charged for every image part, independent of its dimensions.
pub const IMAGE_TOKENS: usize = 256;

/// Portion of a text's cost in tokens.
///
/// Hosts with a real tokenizer inject their own implementation; the framework
/// never ships one, because tokenizer tables are model-specific and heavy.
pub trait TokenEstimator: Send + Sync {
    /// Estimated token count of `text`.
    fn estimate(&self, text: &str) -> usize;
}

/// `chars / chars_per_token`, rounded up.
///
/// Deliberately cheap and deterministic: it over-counts ASCII text slightly and
/// under-counts CJK text badly, which is why it is documented as a heuristic and
/// replaceable through [`ContextAssembler::with_estimator`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeuristicEstimator {
    chars_per_token: usize,
}

impl Default for HeuristicEstimator {
    fn default() -> Self {
        Self {
            chars_per_token: DEFAULT_CHARS_PER_TOKEN,
        }
    }
}

impl HeuristicEstimator {
    /// Builds an estimator with a custom ratio.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when `chars_per_token` is zero.
    pub fn new(chars_per_token: usize) -> CoreResult<Self> {
        if chars_per_token == 0 {
            return Err(CoreError::invalid_argument(
                "context.chars_per_token",
                "must be greater than zero",
            ));
        }
        Ok(Self { chars_per_token })
    }

    /// The configured ratio.
    #[must_use]
    pub const fn chars_per_token(self) -> usize {
        self.chars_per_token
    }
}

impl TokenEstimator for HeuristicEstimator {
    fn estimate(&self, text: &str) -> usize {
        let characters = text.chars().count();
        characters.div_ceil(self.chars_per_token)
    }
}

/// Token budget and compaction behaviour of one assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextPolicy {
    /// Ceiling for the estimated input tokens.
    pub max_input_tokens: usize,
    /// Part of that ceiling reserved for the model's answer.
    pub reserve_output_tokens: usize,
    /// Whether dropped turns are replaced by a one-line omission marker.
    pub emit_omission_marker: bool,
}

impl Default for ContextPolicy {
    fn default() -> Self {
        Self::from_config(&ContextConfig::default())
    }
}

impl ContextPolicy {
    /// Builds a policy from explicit values.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when the ceiling is zero or the
    /// reserve leaves no input budget.
    pub fn new(max_input_tokens: usize, reserve_output_tokens: usize) -> CoreResult<Self> {
        let policy = Self {
            max_input_tokens,
            reserve_output_tokens,
            emit_omission_marker: true,
        };
        policy.validate()?;
        Ok(policy)
    }

    /// Builds a policy from the configuration section.
    #[must_use]
    pub fn from_config(config: &ContextConfig) -> Self {
        Self {
            max_input_tokens: config.max_input_tokens,
            reserve_output_tokens: config.reserve_output_tokens,
            emit_omission_marker: config.emit_omission_marker,
        }
    }

    /// Disables or enables the omission marker.
    #[must_use]
    pub const fn with_omission_marker(mut self, emit: bool) -> Self {
        self.emit_omission_marker = emit;
        self
    }

    /// Tokens available for the input messages.
    #[must_use]
    pub const fn available_input_tokens(self) -> usize {
        self.max_input_tokens
            .saturating_sub(self.reserve_output_tokens)
    }

    /// Re-checks the budget invariants.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when the budget cannot hold a
    /// single input token.
    pub fn validate(self) -> CoreResult<()> {
        if self.max_input_tokens == 0 {
            return Err(CoreError::invalid_argument(
                "context.max_input_tokens",
                "must be greater than zero",
            ));
        }
        if self.reserve_output_tokens >= self.max_input_tokens {
            return Err(CoreError::invalid_argument(
                "context.reserve_output_tokens",
                "must leave at least one input token of the context budget",
            ));
        }
        Ok(())
    }
}

/// Result of one assembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextAssembly {
    /// Messages to send, in replay order.
    pub messages: Vec<Message>,
    /// Number of non-system messages that did not fit.
    pub dropped_messages: usize,
    /// Estimated cost of [`Self::messages`].
    pub estimated_input_tokens: usize,
    /// `true` when pinned system messages alone exceed the input budget.
    pub over_budget: bool,
}

impl ContextAssembly {
    /// Number of messages in the assembly.
    #[must_use]
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Whether the assembly carries no message at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Whether any turn was dropped.
    #[must_use]
    pub fn is_compacted(&self) -> bool {
        self.dropped_messages > 0
    }
}

/// Assembles provider messages under a [`ContextPolicy`].
#[derive(Clone)]
pub struct ContextAssembler {
    policy: ContextPolicy,
    estimator: Arc<dyn TokenEstimator>,
}

impl std::fmt::Debug for ContextAssembler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContextAssembler")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl ContextAssembler {
    /// Builds an assembler with the heuristic estimator.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when the policy is invalid.
    pub fn new(policy: ContextPolicy) -> CoreResult<Self> {
        policy.validate()?;
        Ok(Self {
            policy,
            estimator: Arc::new(HeuristicEstimator::default()),
        })
    }

    /// Replaces the token estimator (for hosts with a real tokenizer).
    #[must_use]
    pub fn with_estimator(mut self, estimator: Arc<dyn TokenEstimator>) -> Self {
        self.estimator = estimator;
        self
    }

    /// The configured policy.
    #[must_use]
    pub const fn policy(&self) -> ContextPolicy {
        self.policy
    }

    /// Assembles `history` under the policy.
    ///
    /// System messages are pinned wherever they appear; the remaining messages
    /// are trimmed from the oldest end.
    #[must_use]
    pub fn assemble(&self, history: &[Message]) -> ContextAssembly {
        let budget = self.policy.available_input_tokens();

        let mut messages = Vec::new();
        let mut tokens = 0;
        for message in history
            .iter()
            .filter(|message| message.role() == Role::System)
        {
            tokens += self.message_tokens(message);
            messages.push(message.clone());
        }

        let others: Vec<&Message> = history
            .iter()
            .filter(|message| message.role() != Role::System)
            .collect();
        let mut start = others.len();
        let mut kept_tokens = 0;
        while start > 0 {
            let cost = self.message_tokens(others[start - 1]);
            if tokens + kept_tokens + cost > budget {
                break;
            }
            kept_tokens += cost;
            start -= 1;
        }
        start = repair_boundary(&others, start);

        // The omission marker is charged against the same budget: make room for
        // it by dropping further turns instead of silently exceeding the ceiling.
        if start > 0 && self.policy.emit_omission_marker {
            while start < others.len() {
                kept_tokens = others[start..]
                    .iter()
                    .map(|message| self.message_tokens(message))
                    .sum();
                let cost = self.message_tokens(&Message::system(omission_text(start)));
                if tokens + kept_tokens + cost <= budget {
                    break;
                }
                start += 1;
                start = repair_boundary(&others, start);
            }
        }

        kept_tokens = others[start..]
            .iter()
            .map(|message| self.message_tokens(message))
            .sum();
        if start > 0 && self.policy.emit_omission_marker {
            let marker = Message::system(omission_text(start));
            let cost = self.message_tokens(&marker);
            if tokens + kept_tokens + cost <= budget {
                tokens += cost;
                messages.push(marker);
            }
        }
        tokens += kept_tokens;

        for message in &others[start..] {
            messages.push((*message).clone());
        }

        ContextAssembly {
            messages,
            dropped_messages: start,
            estimated_input_tokens: tokens,
            over_budget: tokens > budget,
        }
    }

    /// Replays a stored session through [`ContextAssembler::assemble`].
    ///
    /// # Errors
    ///
    /// Propagates [`MemoryStore`] read errors, and
    /// [`CoreError::InvalidArgument`] when `limit` is zero.
    pub fn from_memory(
        &self,
        store: &MemoryStore,
        session: SessionId,
        limit: Option<usize>,
    ) -> CoreResult<ContextAssembly> {
        let stored = store.history(session, limit)?;
        let history: Vec<Message> = stored.iter().map(message_from_memory).collect();
        Ok(self.assemble(&history))
    }

    /// Replays a session, prefixed by its stored compaction summary.
    ///
    /// The summary is written by the host (typically by summarizing the turns it
    /// then prunes) with [`MemoryStore::set_summary`]; it is injected as a system
    /// message right after the existing system instructions, so it survives any
    /// trimming. A summary is a real message and is therefore charged against
    /// the budget: an oversized one makes the assembly report `over_budget`.
    ///
    /// # Errors
    ///
    /// See [`ContextAssembler::from_memory`].
    pub fn from_memory_with_summary(
        &self,
        store: &MemoryStore,
        session: SessionId,
        limit: Option<usize>,
    ) -> CoreResult<ContextAssembly> {
        let stored = store.history(session, limit)?;
        let summary = store.session(session)?.and_then(|session| session.summary);
        let mut history: Vec<Message> = stored.iter().map(message_from_memory).collect();
        if let Some(summary) = summary {
            let position = history
                .iter()
                .take_while(|message| message.role() == Role::System)
                .count();
            history.insert(
                position,
                Message::system(format!("[summary of earlier conversation] {summary}")),
            );
        }
        Ok(self.assemble(&history))
    }

    /// Estimated cost of one message.
    #[must_use]
    pub fn message_tokens(&self, message: &Message) -> usize {
        let mut tokens = MESSAGE_OVERHEAD_TOKENS;
        for part in message.parts() {
            match part {
                ContentPart::Text(text) => tokens += self.estimator.estimate(text),
                ContentPart::Image { url } => {
                    // The URL itself is negligible; the rendered image is not.
                    tokens += IMAGE_TOKENS + self.estimator.estimate(url);
                }
            }
        }
        for call in message.tool_calls() {
            tokens += self.estimator.estimate(&call.name);
            tokens += self.estimator.estimate(&call.arguments);
        }
        tokens
    }
}

/// One-line marker describing how many turns were dropped.
///
/// Count-based and secret-free: the dropped content never appears in the prompt
/// or in a log.
fn omission_text(dropped: usize) -> String {
    format!("[{dropped} earlier message(s) omitted to fit the context budget]")
}

/// Maps one stored message back to a provider message.
fn message_from_memory(message: &MemoryMessage) -> Message {
    match message.role {
        MemoryRole::System => Message::system(message.content.clone()),
        MemoryRole::User => Message::user(message.content.clone()),
        MemoryRole::Assistant => Message::assistant(message.content.clone()),
        MemoryRole::Tool => match &message.tool_call_id {
            Some(call_id) => Message::tool_result(call_id.clone(), message.content.clone()),
            None => Message::tool(message.content.clone()),
        },
    }
}

/// Moves the window start forward until it no longer splits a tool exchange.
fn repair_boundary(messages: &[&Message], mut start: usize) -> usize {
    loop {
        let Some(candidate) = messages.get(start) else {
            return start;
        };
        if candidate.role() == Role::Tool {
            // Orphaned result: the assistant turn that requested it was dropped.
            start += 1;
            continue;
        }
        if candidate.role() == Role::Assistant && !candidate.tool_calls().is_empty() {
            let kept = &messages[start..];
            let complete = candidate.tool_calls().iter().all(|call| {
                kept.iter().any(|message| {
                    message.role() == Role::Tool && message.tool_call_id() == Some(call.id.as_str())
                })
            });
            if !complete {
                start += 1;
                continue;
            }
        }
        return start;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::provider::response::ToolCall;

    fn assembler(max_input_tokens: usize, reserve: usize) -> ContextAssembler {
        ContextAssembler::new(ContextPolicy::new(max_input_tokens, reserve).unwrap()).unwrap()
    }

    fn call(id: &str) -> ToolCall {
        ToolCall {
            id: id.to_owned(),
            name: "echo".to_owned(),
            arguments: "{}".to_owned(),
        }
    }

    fn conversation(turns: usize) -> Vec<Message> {
        (0..turns)
            .map(|index| Message::user(format!("turn {index}")))
            .collect()
    }

    #[test]
    fn system_instructions_are_pinned() {
        let history = vec![
            Message::system("be brief"),
            Message::user("u"),
            Message::assistant("a"),
        ];
        let assembly = assembler(64, 8).assemble(&history);
        assert_eq!(assembly.messages[0].role(), Role::System);
        assert!(assembly.messages.len() > 1);
    }

    #[test]
    fn over_budget_system_is_reported_not_dropped() {
        let long = "x".repeat(400);
        let history = vec![Message::system(long), Message::user("u")];
        let assembly = assembler(16, 4).assemble(&history);
        assert!(assembly.over_budget);
        assert_eq!(assembly.messages.len(), 1);
        assert_eq!(assembly.messages[0].role(), Role::System);
        assert_eq!(assembly.dropped_messages, 1);
    }

    #[test]
    fn keeps_the_newest_turns_and_preserves_order() {
        let history = conversation(50);
        let assembly = assembler(160, 10).assemble(&history);
        assert!(assembly.dropped_messages > 0);
        let kept: Vec<String> = assembly
            .messages
            .iter()
            .map(Message::text_content)
            .collect();
        assert_eq!(kept.last().unwrap(), "turn 49");
        assert!(kept.first().unwrap().contains("omitted"));
        // Order is preserved among kept turns.
        let turns: Vec<&String> = kept
            .iter()
            .filter(|text| text.starts_with("turn "))
            .collect();
        assert!(turns.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(assembly.estimated_input_tokens <= 150);
    }

    #[test]
    fn omission_marker_can_be_disabled() {
        let policy = ContextPolicy::new(40, 8)
            .unwrap()
            .with_omission_marker(false);
        let assembly = ContextAssembler::new(policy)
            .unwrap()
            .assemble(&conversation(20));
        assert!(assembly.is_compacted());
        assert!(assembly
            .messages
            .iter()
            .all(|message| !message.text_content().contains("omitted")));
    }

    #[test]
    fn a_split_tool_exchange_is_dropped_whole() {
        let mut history = Vec::new();
        for index in 0..6 {
            history.push(Message::user(format!("turn {index}")));
        }
        history.push(Message::assistant_tool_calls(
            "",
            vec![call("call_1"), call("call_2")],
        ));
        history.push(Message::tool_result("call_1", "first"));
        history.push(Message::tool_result("call_2", "second"));

        // The budget admits the two results but not the assistant turn.
        let assembly = assembler(20, 4).assemble(&history);
        assert!(assembly.dropped_messages >= 3);
        assert!(!assembly
            .messages
            .iter()
            .any(|message| message.role() == Role::Tool));
        assert!(!assembly
            .messages
            .iter()
            .any(|message| !message.tool_calls().is_empty()));
    }

    #[test]
    fn orphaned_tool_results_are_never_sent() {
        let history = [
            Message::assistant_tool_calls("", vec![call("call_1")]),
            Message::tool_result("call_1", "result"),
            Message::user("next question"),
        ];
        let policy = ContextPolicy::new(200, 8)
            .unwrap()
            .with_omission_marker(false);
        let assembly = ContextAssembler::new(policy)
            .unwrap()
            .assemble(&history[1..]);
        assert_eq!(assembly.messages.len(), 1);
        assert_eq!(assembly.messages[0].role(), Role::User);
        assert_eq!(assembly.dropped_messages, 1);
    }

    #[test]
    fn complete_tool_exchange_is_kept_intact() {
        let history = vec![
            Message::user("question"),
            Message::assistant_tool_calls("", vec![call("call_1")]),
            Message::tool_result("call_1", "result"),
        ];
        let assembly = assembler(400, 8).assemble(&history);
        assert_eq!(assembly.dropped_messages, 0);
        assert_eq!(assembly.messages.len(), 3);
        assert_eq!(assembly.messages[2].tool_call_id(), Some("call_1"));
    }

    #[test]
    fn estimator_ratio_is_configurable() {
        let estimator = HeuristicEstimator::new(2).unwrap();
        assert_eq!(estimator.estimate("abcd"), 2);
        assert_eq!(HeuristicEstimator::default().estimate("abcd"), 1);
        assert_eq!(HeuristicEstimator::default().estimate(""), 0);
        assert!(HeuristicEstimator::new(0).is_err());
    }

    #[test]
    fn budget_validation_rejects_impossible_policies() {
        assert!(ContextPolicy::new(0, 0).is_err());
        assert!(ContextPolicy::new(10, 10).is_err());
        assert!(ContextPolicy::new(10, 11).is_err());
        assert_eq!(
            ContextPolicy::new(10, 4).unwrap().available_input_tokens(),
            6
        );
    }

    #[test]
    fn memory_is_replayed_with_roles_and_tool_ids() {
        let store = MemoryStore::open(":memory:").unwrap();
        let session = SessionId::new();
        use crate::memory::MemoryAppend;
        store
            .append(session, MemoryAppend::new(MemoryRole::System, "policy"))
            .unwrap();
        store
            .append(session, MemoryAppend::new(MemoryRole::User, "question"))
            .unwrap();
        store
            .append(
                session,
                MemoryAppend::new(MemoryRole::Tool, "answer").with_tool_call_id("call_1"),
            )
            .unwrap();

        let assembly = assembler(400, 8)
            .from_memory(&store, session, Some(10))
            .unwrap();
        assert_eq!(assembly.messages.len(), 3);
        assert_eq!(assembly.messages[0].role(), Role::System);
        assert_eq!(assembly.messages[1].role(), Role::User);
        assert_eq!(assembly.messages[2].role(), Role::Tool);
        assert_eq!(assembly.messages[2].tool_call_id(), Some("call_1"));
    }

    #[test]
    fn a_stored_summary_is_injected_after_the_system_instructions() {
        let store = MemoryStore::open(":memory:").unwrap();
        let session = SessionId::new();
        use crate::memory::MemoryAppend;
        store
            .append(session, MemoryAppend::new(MemoryRole::System, "be brief"))
            .unwrap();
        store
            .append(
                session,
                MemoryAppend::new(MemoryRole::User, "latest question"),
            )
            .unwrap();
        store
            .set_summary(session, Some("the user likes apples"))
            .unwrap();

        let assembly = assembler(400, 8)
            .from_memory_with_summary(&store, session, None)
            .unwrap();

        assert_eq!(assembly.message_count(), 3);
        assert_eq!(assembly.messages[0].text_content(), "be brief");
        assert!(assembly.messages[1]
            .text_content()
            .contains("the user likes apples"));
        assert_eq!(assembly.messages[1].role(), Role::System);
        assert_eq!(assembly.messages[2].text_content(), "latest question");

        // Without a summary the replay is unchanged.
        store.set_summary(session, None).unwrap();
        let plain = assembler(400, 8)
            .from_memory_with_summary(&store, session, None)
            .unwrap();
        assert_eq!(plain.message_count(), 2);
    }

    #[test]
    fn assembly_is_deterministic() {
        let history = conversation(30);
        let assembler = assembler(80, 16);
        let first = assembler.assemble(&history);
        let second = assembler.assemble(&history);
        assert_eq!(first, second);
    }
}
