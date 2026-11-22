//! Concrete protocol adapters.
//!
//! | adapter | protocol | covers |
//! |---------|----------|--------|
//! | [`OpenAiCompatibleAdapter`] | OpenAI chat-completions | OpenAI, DeepSeek, Ollama, vLLM, any custom `base_url` |
//! | [`AnthropicAdapter`] | Anthropic messages | Claude models |
//! | [`MockAdapter`] | in-process script | tests, local development, demos |
//!
//! Adding a provider means adding one file here plus a
//! [`crate::provider::ProviderProtocol`] variant; nothing else in the framework
//! changes.

mod anthropic;
mod mock;
mod openai;

pub use self::anthropic::AnthropicAdapter;
pub use self::mock::{MockAdapter, MockFailure, RecordedCall};
pub use self::openai::OpenAiCompatibleAdapter;
