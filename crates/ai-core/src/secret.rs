//! Secret handling and log-safe redaction helpers.
//!
//! Two rules drive this module:
//!
//! 1. **Opt-in disclosure.** A credential is wrapped in [`SecretString`], whose
//!    `Debug`/`Display` implementations always print `[REDACTED]`. The only way
//!    to obtain the value is the explicitly named [`SecretString::expose_secret`].
//!    `Serialize` is intentionally *not* implemented so a secret can never leak
//!    accidentally through a configuration/telemetry serialization path.
//! 2. **Redact by key name.** Hosts frequently log key/value pairs coming from
//!    HTTP headers or JSON bodies. [`redact_for_log`] masks values whose key
//!    looks sensitive (`authorization`, `api_key`, `token`, ...), so a single
//!    logging call cannot turn into a credential leak.
//!
//! This module deliberately has no dependency on `tracing` (the bridge lives in
//! [`crate::observability`]) and never allocates for the masked representation.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

/// Placeholder emitted instead of any secret value.
pub const REDACTED: &str = "[REDACTED]";

/// Markers that always identify a credential (matched case-insensitively against
/// the key name, never the value).
const HARD_SENSITIVE_MARKERS: &[&str] = &[
    "access_token",
    "api-key",
    "api_key",
    "apikey",
    "authorization",
    "bearer",
    "cookie",
    "credential",
    "id_token",
    "passwd",
    "password",
    "private_key",
    "refresh_token",
    "secret",
    "session_token",
    "signature",
];

/// Markers that describe *budgets* rather than credentials, e.g. `max_tokens`.
/// A bare `token` marker does not flag these fields.
const BUDGET_KEY_MARKERS: &[&str] = &[
    "max_", "min_", "num_", "_count", "_limit", "_size", "_tokens", "usage",
];

/// A credential that refuses to be printed.
///
/// # Examples
///
/// ```
/// use ai_core::secret::SecretString;
///
/// let key = SecretString::new("sk-not-a-real-key");
/// assert_eq!(format!("{key}"), "[REDACTED]");
/// assert_eq!(format!("{key:?}"), "SecretString([REDACTED])");
/// assert_eq!(key.expose_secret(), "sk-not-a-real-key");
/// ```
///
/// # Memory hygiene
///
/// The backing `String` is not zeroized on drop (that would require an extra
/// dependency or `unsafe` code). Treat the process memory as trusted; the threat
/// model covered here is accidental *disclosure* through logs, errors, panics and
/// serialized snapshots.
#[derive(Clone, Default)]
pub struct SecretString(String);

impl SecretString {
    /// Placeholder emitted by `Debug`/`Display`.
    pub const REDACTED: &'static str = REDACTED;

    /// Wraps a value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the raw secret.
    ///
    /// The deliberately explicit name documents the intent at every call site:
    /// only transport/authentication boundaries should use this.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// Whether the wrapped value is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of characters in the wrapped value (useful for validation
    /// messages that must not contain the value itself).
    #[must_use]
    pub fn char_count(&self) -> usize {
        self.0.chars().count()
    }

    /// The masked representation, always [`Self::REDACTED`].
    #[must_use]
    pub const fn masked(&self) -> &'static str {
        REDACTED
    }

    /// A short, non-reversible correlation fingerprint.
    ///
    /// Two secrets map to the same fingerprint with probability `2^-64`; the
    /// fingerprint is *not* cryptographic and must never be used as a password
    /// hash. It exists so that logs can tell "same credential as before" apart
    /// from "the credential changed" without revealing the value.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        use std::hash::{Hash, Hasher};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.0.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Reveals a short prefix for interactive diagnostics only.
    ///
    /// Returns [`REDACTED`] when the value is too short to be partially shown.
    /// Never use this for telemetry, metrics or error reporting.
    #[must_use]
    pub fn masked_prefix(&self, visible_prefix: usize) -> String {
        mask_prefix(&self.0, visible_prefix)
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString([REDACTED])")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl PartialEq for SecretString {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(&self.0, &other.0)
    }
}

impl Eq for SecretString {}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl FromStr for SecretString {
    type Err = std::convert::Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.to_owned()))
    }
}

impl<'de> serde::Deserialize<'de> for SecretString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self)
    }
}

/// Compares two strings without an early exit on the first differing byte.
///
/// Length differences are still observable; this only removes the trivially
/// timeable byte-by-byte early return.
fn constant_time_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

/// Whether a key name looks like it carries a credential.
///
/// Matching is case-insensitive and substring based, so `x-api-key`,
/// `Authorization` and `refresh_token` are all detected. Budget fields such as
/// `max_tokens`, `token_limit` or `total_tokens` describe sizes, not credentials,
/// and are deliberately *not* flagged (they are needed in logs).
#[must_use]
pub fn is_sensitive_key(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    if HARD_SENSITIVE_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return true;
    }
    lowered.contains("token")
        && !BUDGET_KEY_MARKERS
            .iter()
            .any(|marker| lowered.contains(marker))
}

/// Returns a log-safe representation of `value`.
///
/// If `key` is sensitive the value is replaced by [`REDACTED`], otherwise the
/// original value is returned without copying.
#[must_use]
pub fn redact_for_log<'a>(key: &str, value: &'a str) -> Cow<'a, str> {
    if is_sensitive_key(key) {
        Cow::Borrowed(REDACTED)
    } else {
        Cow::Borrowed(value)
    }
}

/// Masks a value completely, keeping the type of the return value convenient for
/// formatting (`&'static str`, no allocation).
#[must_use]
pub const fn redact(_value: &str) -> &'static str {
    REDACTED
}

/// Best-effort sanitization of arbitrary text that is about to be logged or
/// embedded in an error message.
///
/// Third-party errors (HTTP clients, OS errors, SDKs) sometimes contain a URL with
/// `?api_key=...`, an `Authorization: Bearer ...` header or a raw `sk-...` key.
/// The framework cannot control those crates, so every root cause is passed
/// through this function before it is rendered by `Display`/`Debug`.
///
/// Three rules are applied:
///
/// 1. `Bearer <token>` → `Bearer [REDACTED]`.
/// 2. Well-known credential prefixes (`sk-`, `ghp_`, `xoxb-`, `AIza`) followed by
///    at least 8 token characters → `[REDACTED]`.
/// 3. `key=value` assignments whose key is sensitive (see [`is_sensitive_key`]) →
///    `key=[REDACTED]`.
///
/// This is defense in depth, **not** a guarantee: the primary rule remains "never
/// put a credential into a message".
#[must_use]
pub fn sanitize_log_text(text: &str) -> String {
    let bearer_masked = mask_bearer_tokens(text);
    let prefixed_masked = mask_prefixed_tokens(&bearer_masked);
    mask_sensitive_assignments(&prefixed_masked)
}

/// Credential prefixes used by common providers.
const TOKEN_PREFIXES: &[&str] = &["sk-", "sk_", "ghp_", "xoxb-", "AIza"];

/// Characters that terminate a token value.
fn is_value_terminator(character: char) -> bool {
    character.is_whitespace() || matches!(character, '"' | '\'' | ',' | ';' | '&' | ')' | ']' | '}')
}

fn mask_bearer_tokens(text: &str) -> String {
    // `to_ascii_lowercase` preserves byte offsets, so indices stay slice-safe.
    let lowered = text.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0_usize;

    while let Some(offset) = lowered[cursor..].find("bearer ") {
        let value_start = cursor + offset + "bearer ".len();
        let value_end = text[value_start..]
            .find(is_value_terminator)
            .map_or(text.len(), |index| value_start + index);
        if value_end > value_start {
            result.push_str(&text[cursor..value_start]);
            result.push_str(REDACTED);
            cursor = value_end;
        } else {
            result.push_str(&text[cursor..value_start]);
            cursor = value_start;
        }
    }
    result.push_str(&text[cursor..]);
    result
}

fn mask_prefixed_tokens(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0_usize;

    loop {
        let remaining = &text[cursor..];
        let found = TOKEN_PREFIXES
            .iter()
            .filter_map(|prefix| remaining.find(prefix).map(|index| (index, *prefix)))
            .min_by_key(|(index, _)| *index);
        let Some((offset, prefix)) = found else {
            result.push_str(remaining);
            return result;
        };

        let start = cursor + offset;
        let token_start = start + prefix.len();
        let token_end = text[token_start..]
            .find(|character: char| {
                !(character.is_ascii_alphanumeric() || character == '-' || character == '_')
            })
            .map_or(text.len(), |index| token_start + index);

        result.push_str(&text[cursor..start]);
        if token_end.saturating_sub(token_start) >= 8 {
            result.push_str(REDACTED);
        } else {
            result.push_str(&text[start..token_end]);
        }
        cursor = token_end;
        if cursor >= text.len() {
            return result;
        }
    }
}

fn mask_sensitive_assignments(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0_usize;

    while let Some(offset) = text[cursor..].find('=') {
        let equals = cursor + offset;
        let key_start = text[..equals]
            .rfind(|character: char| {
                !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
            })
            .map_or(0, |index| index + 1);
        let key = &text[key_start..equals];

        if key_start >= cursor && !key.is_empty() && is_sensitive_key(key) {
            let value_start = equals + 1;
            let value_end = text[value_start..]
                .find(is_value_terminator)
                .map_or(text.len(), |index| value_start + index);
            result.push_str(&text[cursor..value_start]);
            result.push_str(REDACTED);
            cursor = value_end;
        } else {
            result.push_str(&text[cursor..=equals]);
            cursor = equals + 1;
        }
        if cursor >= text.len() {
            break;
        }
    }
    result.push_str(&text[cursor..]);
    result
}

/// Keeps a short prefix of a long value and masks the remainder.
///
/// Values shorter than `visible_prefix + 4` characters are fully masked, because
/// showing "most" of a short token is equivalent to showing all of it.
#[must_use]
pub fn mask_prefix(value: &str, visible_prefix: usize) -> String {
    let characters: Vec<char> = value.chars().collect();
    if characters.len() <= visible_prefix.saturating_add(4) {
        return REDACTED.to_owned();
    }
    let prefix: String = characters.iter().take(visible_prefix).collect();
    format!("{prefix}***")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_never_contain_the_secret() {
        let secret = SecretString::new("sk-live-0123456789abcdef");
        assert_eq!(secret.to_string(), REDACTED);
        assert_eq!(format!("{secret:?}"), "SecretString([REDACTED])");
        assert!(!format!("{secret:?}").contains("sk-live"));
        assert_eq!(secret.expose_secret(), "sk-live-0123456789abcdef");
        assert_eq!(secret.char_count(), 24);
        assert!(!secret.is_empty());
    }

    #[test]
    fn equality_is_value_based() {
        let left = SecretString::new("same-value");
        let right = SecretString::new("same-value");
        let other = SecretString::new("other-value");
        assert_eq!(left, right);
        assert_ne!(left, other);
    }

    #[test]
    fn fingerprint_is_stable_and_non_reversible() {
        let secret = SecretString::new("sk-live-0123456789abcdef");
        assert_eq!(
            secret.fingerprint(),
            SecretString::new("sk-live-0123456789abcdef").fingerprint()
        );
        assert_ne!(
            secret.fingerprint(),
            SecretString::new("other").fingerprint()
        );
        assert!(!secret.fingerprint().contains("sk-live"));
        assert_eq!(secret.fingerprint().len(), 16);
    }

    #[test]
    fn sensitive_keys_are_detected() {
        for key in [
            "Authorization",
            "x-api-key",
            "refresh_token",
            "client_secret",
            "user.password",
            "Cookie",
        ] {
            assert!(is_sensitive_key(key), "{key} should be sensitive");
        }
        for key in ["model", "user_id", "trace_id", "max_tokens"] {
            assert!(!is_sensitive_key(key), "{key} should not be sensitive");
        }
    }

    #[test]
    fn redact_for_log_masks_sensitive_values_only() {
        assert_eq!(
            redact_for_log("authorization", "Bearer abc").as_ref(),
            REDACTED
        );
        assert_eq!(
            redact_for_log("model", "gpt-4o-mini").as_ref(),
            "gpt-4o-mini"
        );
        assert_eq!(redact("anything"), REDACTED);
    }

    #[test]
    fn mask_prefix_hides_short_values_completely() {
        assert_eq!(mask_prefix("short", 4), REDACTED);
        assert_eq!(mask_prefix("sk-live-0123456789abcdef", 7), "sk-live***");
        assert_eq!(
            SecretString::new("sk-live-0123456789abcdef").masked_prefix(3),
            "sk-***"
        );
        assert_eq!(SecretString::new("abc").masked_prefix(3), REDACTED);
    }

    #[test]
    fn deserialization_is_supported_but_not_serialization() {
        let secret: SecretString = serde_json::from_str("\"value\"").unwrap();
        assert_eq!(secret.expose_secret(), "value");
    }

    #[test]
    fn sanitize_masks_bearer_tokens() {
        assert_eq!(
            sanitize_log_text("Authorization: Bearer abc123"),
            "Authorization: Bearer [REDACTED]"
        );
    }

    #[test]
    fn sanitize_masks_known_credential_prefixes() {
        let cleaned = sanitize_log_text("request failed with sk-live-0123456789abcdef");
        assert!(!cleaned.contains("sk-live"));
        assert!(cleaned.contains("[REDACTED]"));

        // Not long enough to be a credential: left untouched.
        assert_eq!(sanitize_log_text("disk-usage"), "disk-usage");
    }

    #[test]
    fn sanitize_masks_sensitive_assignments_only() {
        assert_eq!(
            sanitize_log_text("api_key=abc&model=gpt-4o-mini"),
            "api_key=[REDACTED]&model=gpt-4o-mini"
        );
        assert_eq!(sanitize_log_text("max_tokens=512"), "max_tokens=512");
    }

    #[test]
    fn sanitize_is_idempotent_for_clean_text() {
        let clean = "provider openai returned status 503";
        assert_eq!(sanitize_log_text(clean), clean);
        assert_eq!(sanitize_log_text(&sanitize_log_text(clean)), clean);
    }
}
