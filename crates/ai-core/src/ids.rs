//! Strongly typed, `Copy` identifiers used across the framework.
//!
//! Raw `String`/`u64` identifiers are easy to mix up: passing a session id where
//! a request id is expected silently compiles. The newtypes in this module make
//! that a type error, cost nothing at runtime (128-bit inline values) and can be
//! used directly as `HashMap`/`BTreeMap` keys or as `tracing` span fields.
//!
//! All identifiers are 128-bit values rendered as 32 lowercase hexadecimal
//! characters. They are generated from a process-unique random seed combined
//! with a monotonic counter and a nanosecond timestamp: the counter guarantees
//! uniqueness inside the process, the random seed makes collisions across
//! processes and runs practically impossible.
//!
//! These values are **correlation identifiers, not secrets**: they carry no
//! authentication weight and are safe (and expected) to log.
//!
//! # Trait choices
//!
//! The identifiers are `Copy` and allocation-free, so `AsRef<str>` is
//! intentionally *not* implemented (it would require storing a `String`).
//! Use [`fmt::Display`] / [`RequestId::to_hex`] for text and
//! [`RequestId::as_bytes`] for the raw bytes.

use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{CoreError, CoreResult};

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
const HEX_LEN: usize = 32;

/// Generates a fresh 128-bit correlation value.
fn random_u128() -> u128 {
    static RANDOM: OnceLock<(RandomState, RandomState)> = OnceLock::new();
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let (first, second) = RANDOM.get_or_init(|| (RandomState::new(), RandomState::new()));
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0);

    let mut high = first.build_hasher();
    high.write_u64(counter);
    high.write_u64(nanos);
    high.write_u32(std::process::id());

    let mut low = second.build_hasher();
    low.write_u64(nanos);
    low.write_u64(counter);
    low.write_u64(0x9E37_79B9_7F4A_7C15);

    (u128::from(high.finish()) << 64) | u128::from(low.finish())
}

fn encode_hex(value: u128) -> String {
    let mut buffer = [0_u8; HEX_LEN];
    for (index, slot) in buffer.iter_mut().enumerate() {
        let shift = 4 * (HEX_LEN - 1 - index);
        let nibble = u8::try_from((value >> shift) & 0xF).unwrap_or(0);
        *slot = HEX_DIGITS[usize::from(nibble)];
    }
    // The buffer only ever contains ASCII hex digits, so this cannot fail; the
    // fallible match exists to keep the crate `unsafe`-free.
    match std::str::from_utf8(&buffer) {
        Ok(text) => text.to_owned(),
        Err(_) => String::new(),
    }
}

fn decode_hex(text: &str) -> Result<u128, &'static str> {
    let bytes = text.as_bytes();
    if bytes.len() != HEX_LEN {
        return Err("expected exactly 32 hexadecimal characters");
    }
    let mut value = 0_u128;
    for byte in bytes {
        let nibble = match byte {
            b'0'..=b'9' => u128::from(byte - b'0'),
            b'a'..=b'f' => u128::from(byte - b'a') + 10,
            b'A'..=b'F' => u128::from(byte - b'A') + 10,
            _ => return Err("expected exactly 32 hexadecimal characters"),
        };
        value = (value << 4) | nibble;
    }
    Ok(value)
}

/// Declares a `Copy` 128-bit identifier newtype with the full public trait set.
macro_rules! define_id {
    (
        $(#[$meta:meta])*
        $name:ident,
        $field:literal
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u128);

        impl $name {
            /// Generates a fresh identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(random_u128())
            }

            /// Rebuilds an identifier from its raw 128-bit representation.
            #[must_use]
            pub const fn from_u128(value: u128) -> Self {
                Self(value)
            }

            /// Rebuilds an identifier from big-endian bytes.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(u128::from_be_bytes(bytes))
            }

            /// Returns the raw 128-bit representation.
            #[must_use]
            pub const fn as_u128(&self) -> u128 {
                self.0
            }

            /// Returns the big-endian byte representation.
            #[must_use]
            pub const fn as_bytes(&self) -> [u8; 16] {
                self.0.to_be_bytes()
            }

            /// Returns the canonical 32-character lowercase hex form.
            #[must_use]
            pub fn to_hex(&self) -> String {
                encode_hex(self.0)
            }

            /// Parses the canonical hex form.
            ///
            /// # Errors
            ///
            /// Returns [`CoreError::InvalidArgument`] when `text` is not exactly
            /// 32 hexadecimal characters.
            pub fn parse(text: &str) -> CoreResult<Self> {
                decode_hex(text)
                    .map(Self)
                    .map_err(|reason| CoreError::invalid_argument($field, reason))
            }
        }

        impl Default for $name {
            /// Generates a fresh identifier (same as [`Self::new`]).
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&encode_hex(self.0))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), encode_hex(self.0))
            }
        }

        impl FromStr for $name {
            type Err = CoreError;

            fn from_str(text: &str) -> CoreResult<Self> {
                Self::parse(text)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                Self::parse(&raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_id!(
    /// Identifies one logical AI request end to end.
    ///
    /// A single user action may fan out into several provider calls; each call
    /// gets its own `RequestId` while sharing the parent [`TraceId`].
    RequestId,
    "request_id"
);

define_id!(
    /// Identifies a whole trace (user action / conversation turn).
    ///
    /// Propagated to every child request, span and log event so that
    /// observability backends can correlate the full causal chain.
    TraceId,
    "trace_id"
);

define_id!(
    /// Identifies a client conversation session.
    ///
    /// Sessions outlive individual traces; it is optional everywhere because the
    /// framework must stay usable in headless/test contexts.
    SessionId,
    "session_id"
);

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn display_and_parse_round_trip() {
        let id = RequestId::new();
        let text = id.to_string();
        assert_eq!(text.len(), 32);
        assert!(text
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
        assert_eq!(RequestId::parse(&text).unwrap(), id);
        assert_eq!(RequestId::from_str(&text).unwrap(), id);
    }

    #[test]
    fn parse_is_case_insensitive_and_bounds_checked() {
        let raw = "0123456789abcdef0123456789ABCDEF";
        let parsed = TraceId::parse(raw).unwrap();
        assert_eq!(parsed.to_hex(), "0123456789abcdef0123456789abcdef");

        let too_short = TraceId::parse("abcd");
        assert!(matches!(too_short, Err(CoreError::InvalidArgument { .. })));

        let bad_char = SessionId::parse("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
        assert!(bad_char.is_err());
    }

    #[test]
    fn identifiers_are_unique_across_threads() {
        let handles: Vec<_> = (0..8)
            .map(|_| std::thread::spawn(|| (0..500).map(|_| RequestId::new()).collect::<Vec<_>>()))
            .collect();

        let mut all = HashSet::new();
        for handle in handles {
            for id in handle.join().unwrap() {
                assert!(all.insert(id), "duplicate identifier generated");
            }
        }
        assert_eq!(all.len(), 8 * 500);
    }

    #[test]
    fn byte_round_trip() {
        let id = SessionId::new();
        assert_eq!(SessionId::from_bytes(id.as_bytes()), id);
        assert_eq!(SessionId::from_u128(id.as_u128()), id);
        assert_eq!(u128::from_be_bytes(id.as_bytes()), id.as_u128());
    }

    #[test]
    fn serde_uses_hex_representation() {
        let id = RequestId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<RequestId>(&json).unwrap(), id);
        assert!(serde_json::from_str::<RequestId>("\"nope\"").is_err());
    }
}
