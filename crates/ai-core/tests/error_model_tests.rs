//! Verifies the public error contract: classification, codes, retryability,
//! display output and root-cause preservation.

use std::error::Error as StdError;
use std::time::Duration;

use ai_core::{CoreError, ErrorKind, StorageArea};

#[test]
fn every_variant_has_a_stable_kind_and_code() {
    let cases: Vec<(CoreError, ErrorKind, &str)> = vec![
        (CoreError::config("bad"), ErrorKind::Config, "CORE_CONFIG"),
        (
            CoreError::invalid_argument("field", "bad"),
            ErrorKind::InvalidArgument,
            "CORE_INVALID_ARGUMENT",
        ),
        (
            CoreError::timeout("op", Duration::from_millis(5)),
            ErrorKind::Timeout,
            "CORE_TIMEOUT",
        ),
        (
            CoreError::cancelled("op"),
            ErrorKind::Cancelled,
            "CORE_CANCELLED",
        ),
        (
            CoreError::from(std::io::Error::other("boom")),
            ErrorKind::Io,
            "CORE_IO",
        ),
        (
            CoreError::serialization("bad json"),
            ErrorKind::Serialization,
            "CORE_SERIALIZATION",
        ),
        (
            CoreError::not_found("thing"),
            ErrorKind::NotFound,
            "CORE_NOT_FOUND",
        ),
        (
            CoreError::concurrency("join"),
            ErrorKind::Concurrency,
            "CORE_CONCURRENCY",
        ),
        (
            CoreError::internal("bug"),
            ErrorKind::Internal,
            "CORE_INTERNAL",
        ),
        (CoreError::Shutdown, ErrorKind::Shutdown, "CORE_SHUTDOWN"),
        (
            CoreError::Provider {
                provider: "demo".to_owned(),
                message: "upstream 503".to_owned(),
                retryable: true,
                source: None,
            },
            ErrorKind::Provider,
            "CORE_PROVIDER",
        ),
        (
            CoreError::Authentication {
                message: "missing credential".to_owned(),
            },
            ErrorKind::Authentication,
            "CORE_AUTHENTICATION",
        ),
        (
            CoreError::unsupported_capability("vision"),
            ErrorKind::CapabilityUnsupported,
            "CORE_CAPABILITY_UNSUPPORTED",
        ),
        (
            CoreError::TokenLimitExceeded {
                message: "budget".to_owned(),
            },
            ErrorKind::TokenLimitExceeded,
            "CORE_TOKEN_LIMIT",
        ),
        (
            CoreError::Tool {
                tool: "calc".to_owned(),
                message: "bad args".to_owned(),
            },
            ErrorKind::Tool,
            "CORE_TOOL",
        ),
        (
            CoreError::storage(StorageArea::Memory, "disk full", None),
            ErrorKind::Storage,
            "CORE_STORAGE",
        ),
    ];

    for (error, expected_kind, expected_code) in cases {
        assert_eq!(error.kind(), expected_kind, "kind mismatch for {error:?}");
        assert_eq!(error.error_code(), expected_code);
        let rendered = error.to_string();
        assert!(!rendered.is_empty(), "display must not be empty");
        assert!(
            !rendered.contains("CoreError"),
            "display must be human readable, got `{rendered}`"
        );
        assert_eq!(error.to_string(), rendered, "display must be stable");
    }
}

#[test]
fn classification_helpers_are_consistent() {
    assert!(CoreError::cancelled("op").is_cancelled());
    assert!(!CoreError::timeout("op", Duration::from_secs(1)).is_cancelled());
    assert!(CoreError::timeout("op", Duration::from_secs(1)).is_timeout());

    assert!(CoreError::timeout("op", Duration::from_secs(1)).is_retryable());
    assert!(CoreError::concurrency("race").is_retryable());
    assert!(!CoreError::cancelled("op").is_retryable());
    assert!(!CoreError::invalid_argument("f", "m").is_retryable());
    assert!(!CoreError::Shutdown.is_retryable());
}

#[test]
fn io_retryability_depends_on_the_underlying_kind() {
    let transient = CoreError::from(std::io::Error::new(
        std::io::ErrorKind::ConnectionReset,
        "reset",
    ));
    assert!(transient.is_retryable());

    let permanent = CoreError::from(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "denied",
    ));
    assert!(!permanent.is_retryable());
}

#[test]
fn provider_retryability_is_carried_per_instance() {
    let retryable = CoreError::Provider {
        provider: "demo".to_owned(),
        message: "429".to_owned(),
        retryable: true,
        source: None,
    };
    assert!(retryable.is_retryable());

    let fatal = CoreError::Provider {
        provider: "demo".to_owned(),
        message: "400".to_owned(),
        retryable: false,
        source: None,
    };
    assert!(!fatal.is_retryable());
}

#[test]
fn source_chain_is_preserved() {
    let root = std::io::Error::new(std::io::ErrorKind::NotFound, "missing file");
    let error = CoreError::config_with_source("cannot load config", root);

    let source = error.source().expect("source must be preserved");
    assert!(source.to_string().contains("missing file"));
    assert_eq!(error.kind(), ErrorKind::Config);

    let serialization =
        CoreError::serialization_with_source("cannot parse", std::io::Error::other("inner detail"));
    assert!(serialization.source().is_some());
}

#[test]
fn top_level_display_never_inlines_the_root_cause() {
    // A root cause may contain whatever a third-party library decided to print;
    // the framework's own message must stay a short, redacted summary.
    let leaky_source = std::io::Error::other("Authorization: Bearer sk-live-should-not-appear");
    let error = CoreError::config_with_source("upstream rejected configuration", leaky_source);

    let rendered = error.to_string();
    assert!(!rendered.contains("sk-live-should-not-appear"));
    assert!(!format!("{error:?}").contains("sk-live-should-not-appear"));
}

#[test]
fn storage_area_is_part_of_the_error_contract() {
    let error = CoreError::storage(StorageArea::Cache, "purge failed", None);
    assert_eq!(
        error.to_string(),
        "storage error in `ai-cache`: purge failed"
    );
    assert_eq!(StorageArea::Memory.dir_name(), "ai-memory");
    assert_eq!(StorageArea::Metadata.to_string(), "ai-metadata");
}

#[test]
fn errors_are_send_sync_static() {
    fn assert_bounds<T: Send + Sync + 'static>() {}
    assert_bounds::<CoreError>();
    assert_bounds::<ErrorKind>();

    let boxed: Box<dyn StdError + Send + Sync> = Box::new(CoreError::internal("boxed"));
    assert!(boxed.to_string().contains("boxed"));
}
