//! Shared HTTP plumbing for the real adapters.
//!
//! Kept `pub(crate)` on purpose: no `reqwest` type may appear in the public API,
//! so hosts cannot couple to the transport implementation and the transport can
//! be replaced without a breaking change.
//!
//! Everything that leaves this module as an error message passes through
//! [`sanitize_log_text`], because provider error bodies are third-party text that
//! may echo an `Authorization` header or a key-shaped string.

use reqwest::{Client, StatusCode};

use crate::error::CoreError;
use crate::provider::config::ProviderId;
use crate::secret::sanitize_log_text;
use crate::CoreResult;

/// `User-Agent` sent with every provider request.
pub(crate) const USER_AGENT: &str = concat!("sage-ai-core/", env!("CARGO_PKG_VERSION"));

/// Maximum number of characters of a provider error body kept in the message.
const BODY_SNIPPET_CHARS: usize = 400;

/// Builds the shared HTTP client.
///
/// `rustls` (ring provider) is used instead of the platform TLS stack so the
/// framework builds identically on Windows/macOS/Linux without OpenSSL.
pub(crate) fn build_client() -> CoreResult<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| {
            CoreError::internal(format!(
                "failed to build the provider HTTP client: {}",
                sanitize_log_text(&error.to_string())
            ))
        })
}

/// Shortens and sanitizes a provider error body for inclusion in a message.
pub(crate) fn body_snippet(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let total = trimmed.chars().count();
    let clipped: String = trimmed.chars().take(BODY_SNIPPET_CHARS).collect();
    let sanitized = sanitize_log_text(&clipped);
    if total > BODY_SNIPPET_CHARS {
        format!("{sanitized}…")
    } else {
        sanitized
    }
}

/// Heuristics for provider-side "context too long" errors.
///
/// Providers return HTTP 400 for these, so the body has to be inspected to map
/// them to [`CoreError::TokenLimitExceeded`] instead of a generic client error.
fn looks_like_token_limit(body: &str) -> bool {
    let lowered = body.to_ascii_lowercase();
    [
        "context_length_exceeded",
        "maximum context length",
        "context window",
        "too many tokens",
        "prompt is too long",
        "input is too long",
        "reduce the length",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

/// Whether an HTTP status is worth retrying (rate limit, transient server error).
fn is_retryable_status(status: StatusCode) -> bool {
    let code = status.as_u16();
    matches!(code, 408 | 425 | 429 | 529) || status.is_server_error()
}

/// Maps a non-success HTTP response onto the unified error model.
///
/// * 401/403 → [`CoreError::Authentication`] (never retried)
/// * 400/413/422 with a context-length signature → [`CoreError::TokenLimitExceeded`]
/// * 408/425/429/529/5xx → [`CoreError::Provider`] with `retryable = true`
/// * other 4xx → [`CoreError::Provider`] with `retryable = false`
pub(crate) fn status_error(
    provider: &ProviderId,
    model: &str,
    status: StatusCode,
    body: &str,
) -> CoreError {
    let code = status.as_u16();
    let snippet = body_snippet(body);
    let detail = if snippet.is_empty() {
        String::new()
    } else {
        format!(": {snippet}")
    };

    if matches!(code, 401 | 403) {
        return CoreError::authentication(format!(
            "provider=`{provider}` model=`{model}` rejected the credentials (HTTP {code}){detail}"
        ));
    }
    if matches!(code, 400 | 413 | 422) && looks_like_token_limit(body) {
        return CoreError::token_limit(format!(
            "provider=`{provider}` model=`{model}` reported a context/token limit (HTTP {code}){detail}"
        ));
    }
    CoreError::provider(
        provider.to_string(),
        format!("provider=`{provider}` model=`{model}` returned HTTP {code}{detail}"),
        is_retryable_status(status),
    )
}

/// Maps a transport failure (DNS, connect, TLS, body read) onto the unified model.
pub(crate) fn network_error(
    provider: &ProviderId,
    model: &str,
    error: reqwest::Error,
) -> CoreError {
    let retryable =
        error.is_timeout() || error.is_connect() || error.is_request() || error.is_body();
    let message = format!(
        "provider=`{provider}` model=`{model}` transport failure: {}",
        sanitize_log_text(&error.to_string())
    );
    CoreError::provider_with_source(provider.to_string(), message, retryable, error)
}

/// Maps a JSON decoding failure of a provider payload.
pub(crate) fn decode_error(
    provider: &ProviderId,
    model: &str,
    what: &str,
    error: serde_json::Error,
) -> CoreError {
    CoreError::serialization_with_source(
        format!("provider=`{provider}` model=`{model}` returned an invalid {what}"),
        error,
    )
}

/// Incremental Server-Sent-Events line decoder.
///
/// Bytes are buffered until a full `\n`-terminated line is available, so a UTF-8
/// character split across two chunks is never corrupted (a newline byte can never
/// appear inside a multi-byte UTF-8 sequence).
#[derive(Debug, Default)]
pub(crate) struct SseDecoder {
    buffer: Vec<u8>,
}

impl SseDecoder {
    /// Creates an empty decoder.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Appends raw bytes received from the wire.
    pub(crate) fn push(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    /// Returns the payload of the next `data:` line, if one is complete.
    ///
    /// Comment lines (`: keep-alive`), `event:`, `id:` and empty separators are
    /// skipped, which is what OpenAI/DeepSeek and Anthropic streams all require.
    pub(crate) fn next_event(&mut self) -> Option<String> {
        loop {
            let newline = self.buffer.iter().position(|byte| *byte == b'\n')?;
            let line: Vec<u8> = self.buffer.drain(..=newline).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            if let Some(data) = line.strip_prefix("data:") {
                return Some(data.trim_start().to_owned());
            }
            // `event:` / `id:` / `retry:` carry metadata we do not need; the
            // payload always repeats the type inside the JSON body.
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn sse_decoder_handles_partial_lines_and_comments() {
        let mut decoder = SseDecoder::new();
        decoder.push(b": keep-alive\ndata: {\"a\":1}\n\n");
        assert_eq!(decoder.next_event().as_deref(), Some("{\"a\":1}"));
        assert_eq!(decoder.next_event(), None);

        decoder.push(b"data: {\"b\"");
        assert_eq!(decoder.next_event(), None);
        decoder.push(b":2}\ndata: [DONE]\n");
        assert_eq!(decoder.next_event().as_deref(), Some("{\"b\":2}"));
        assert_eq!(decoder.next_event().as_deref(), Some("[DONE]"));
    }

    #[test]
    fn sse_decoder_handles_crlf_and_multibyte_split() {
        let mut decoder = SseDecoder::new();
        let payload = "data: {\"t\":\"学习\"}\r\n".as_bytes().to_vec();
        let split = payload.len() - 3;
        decoder.push(&payload[..split]);
        assert_eq!(decoder.next_event(), None);
        decoder.push(&payload[split..]);
        assert_eq!(decoder.next_event().as_deref(), Some("{\"t\":\"学习\"}"));
    }

    #[test]
    fn status_mapping_matches_the_error_contract() {
        let provider = ProviderId::new("mock").unwrap();

        let auth = status_error(&provider, "m", StatusCode::UNAUTHORIZED, "invalid key");
        assert!(matches!(auth, CoreError::Authentication { .. }));
        assert!(!auth.is_retryable());

        let limited = status_error(&provider, "m", StatusCode::TOO_MANY_REQUESTS, "slow down");
        assert!(matches!(
            limited,
            CoreError::Provider {
                retryable: true,
                ..
            }
        ));

        let server = status_error(&provider, "m", StatusCode::SERVICE_UNAVAILABLE, "down");
        assert!(server.is_retryable());

        let client = status_error(&provider, "m", StatusCode::BAD_REQUEST, "bad");
        assert!(matches!(
            client,
            CoreError::Provider {
                retryable: false,
                ..
            }
        ));

        let tokens = status_error(
            &provider,
            "m",
            StatusCode::BAD_REQUEST,
            "This model's maximum context length is 8192 tokens",
        );
        assert!(matches!(tokens, CoreError::TokenLimitExceeded { .. }));
    }

    #[test]
    fn error_bodies_are_sanitized_and_clipped() {
        let provider = ProviderId::new("mock").unwrap();
        let error = status_error(
            &provider,
            "m",
            StatusCode::BAD_REQUEST,
            "Authorization: Bearer sk-live-0123456789abcdef",
        );
        let message = error.to_string();
        assert!(!message.contains("sk-live-0123456789abcdef"));
        assert!(message.contains("[REDACTED]"));

        let long = "x".repeat(1_000);
        let snippet = body_snippet(&long);
        assert!(snippet.ends_with('…'));
        assert!(snippet.chars().count() <= BODY_SNIPPET_CHARS + 1);
    }
}
