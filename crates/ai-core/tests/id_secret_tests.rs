//! Strongly typed identifiers and secret redaction guarantees.

use std::collections::HashSet;
use std::str::FromStr;

use ai_core::observability::{is_sensitive_key, mask_prefix, redact_for_log, SecretString};
use ai_core::{RequestId, SessionId, TraceId};

#[test]
fn identifiers_are_unique_and_round_trip() {
    let id = RequestId::new();
    let text = id.to_string();

    assert_eq!(text.len(), 32);
    assert_eq!(RequestId::parse(&text).expect("parse"), id);
    assert_eq!(RequestId::from_str(&text).expect("from_str"), id);
    assert_eq!(
        RequestId::from_bytes(id.as_bytes()).to_string(),
        text,
        "byte round trip must preserve the identifier"
    );
    assert_eq!(RequestId::from_u128(id.as_u128()), id);
}

#[test]
fn identifier_types_are_distinct_newtypes() {
    // The value of the newtypes is enforced by the compiler: each field below only
    // accepts its own type, so identifiers cannot be swapped by accident.
    struct Bundle {
        trace: TraceId,
        request: RequestId,
        session: Option<SessionId>,
    }

    let bundle = Bundle {
        trace: TraceId::new(),
        request: RequestId::new(),
        session: Some(SessionId::new()),
    };
    assert_eq!(bundle.trace.to_string().len(), 32);
    assert_eq!(bundle.request.to_string().len(), 32);
    assert!(bundle.session.is_some());
}

#[test]
fn malformed_identifiers_are_rejected() {
    let long_g = "g".repeat(32);
    let short = "0".repeat(31);
    for candidate in ["", "xyz", long_g.as_str(), short.as_str()] {
        let error = RequestId::parse(candidate).expect_err("must be rejected");
        assert!(error.to_string().contains("request_id"));
    }
    assert!(RequestId::parse(&"A".repeat(32)).is_ok());
}

#[test]
fn identifiers_work_as_map_keys() {
    let mut seen = HashSet::new();
    for _ in 0..1_000 {
        assert!(seen.insert(RequestId::new()));
    }
    assert_eq!(seen.len(), 1_000);
    assert!(seen.contains(&seen.iter().next().copied().expect("non-empty")));
}

#[test]
fn identifiers_serialize_as_hex_strings() {
    let id = TraceId::new();
    let json = serde_json::to_string(&id).expect("serializable");
    assert_eq!(json, format!("\"{id}\""));
    assert_eq!(serde_json::from_str::<TraceId>(&json).expect("parse"), id);
    assert!(serde_json::from_str::<TraceId>("\"short\"").is_err());
}

#[test]
fn secret_values_never_leak_through_formatting() {
    let secret = SecretString::new("sk-live-0123456789abcdef");

    assert_eq!(secret.to_string(), "[REDACTED]");
    assert_eq!(format!("{secret:?}"), "SecretString([REDACTED])");
    assert!(!format!("{secret:?}").contains("sk-live"));
    assert!(!format!("{secret}").contains("sk-live"));
    assert_eq!(secret.masked(), "[REDACTED]");
    assert_eq!(secret.expose_secret(), "sk-live-0123456789abcdef");
}

#[test]
fn secret_fingerprint_is_useful_but_not_revealing() {
    let first = SecretString::new("sk-live-0123456789abcdef");
    let second = SecretString::new("sk-live-0123456789abcdef");
    let other = SecretString::new("sk-live-fedcba9876543210");

    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_ne!(first.fingerprint(), other.fingerprint());
    assert!(!first.fingerprint().contains("sk-live"));
    assert_eq!(first, second);
    assert_ne!(first, other);
}

#[test]
fn secret_values_are_not_serializable() {
    // `Serialize` is intentionally not implemented: this test documents that the
    // only serializable direction is deserialization (loading a credential).
    let loaded: SecretString = serde_json::from_str("\"sk-test\"").expect("deserializable");
    assert_eq!(loaded.expose_secret(), "sk-test");
}

#[test]
fn sensitive_keys_are_masked_in_key_value_logs() {
    assert!(is_sensitive_key("Authorization"));
    assert!(is_sensitive_key("x-api-key"));
    assert!(is_sensitive_key("refresh_token"));
    assert!(!is_sensitive_key("model"));

    assert_eq!(
        redact_for_log("authorization", "Bearer sk-live-xyz").as_ref(),
        "[REDACTED]"
    );
    assert_eq!(
        redact_for_log("model", "gpt-4o-mini").as_ref(),
        "gpt-4o-mini"
    );
}

#[test]
fn partial_masking_only_reveals_long_values() {
    assert_eq!(mask_prefix("tiny", 4), "[REDACTED]");
    assert_eq!(mask_prefix("sk-live-0123456789abcdef", 7), "sk-live***");
    assert_eq!(
        SecretString::new("sk-live-0123456789abcdef").masked_prefix(7),
        "sk-live***"
    );
}

#[test]
fn empty_secret_is_reported_as_empty_without_value() {
    let empty = SecretString::default();
    assert!(empty.is_empty());
    assert_eq!(empty.char_count(), 0);
    assert_eq!(empty.to_string(), "[REDACTED]");
}
