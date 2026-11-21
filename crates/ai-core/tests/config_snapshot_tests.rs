//! Configuration validation and immutable-snapshot semantics.

mod common;

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use ai_core::config::{
    AiCoreConfig, ConfigVersion, LogLevel, MAX_BLOCKING_TASKS, MAX_CONCURRENT_REQUESTS,
};
use ai_core::{ConfigStore, CoreError, ErrorKind};

#[test]
fn default_configuration_is_valid() {
    let config = AiCoreConfig::default();
    assert!(config.validate().is_ok());

    let layout = config.storage_layout().expect("default root must be valid");
    assert!(layout.memory().ends_with("ai-memory"));
    assert!(layout.cache().ends_with("ai-cache"));
}

#[test]
fn invalid_configurations_are_rejected_with_the_offending_field() {
    let mut zero_request = AiCoreConfig::default();
    zero_request.timeouts.request = Duration::ZERO;
    let error = zero_request.validate().expect_err("zero timeout must fail");
    assert_eq!(error.kind(), ErrorKind::Config);
    assert!(error.to_string().contains("timeouts.request"));

    let mut zero_connect = AiCoreConfig::default();
    zero_connect.timeouts.connect = Duration::ZERO;
    assert!(zero_connect.validate().is_err());

    let mut zero_concurrency = AiCoreConfig::default();
    zero_concurrency.concurrency.max_concurrent_requests = 0;
    assert!(zero_concurrency.validate().is_err());

    let mut too_many = AiCoreConfig::default();
    too_many.concurrency.max_blocking_tasks = MAX_BLOCKING_TASKS + 1;
    assert!(too_many.validate().is_err());

    let mut escaping_root = AiCoreConfig::default();
    escaping_root.storage.root = "../outside".into();
    let error = escaping_root
        .storage_layout()
        .expect_err("`..` must be rejected");
    assert!(error.to_string().contains("storage.root"));

    let mut empty_service = AiCoreConfig::default();
    empty_service.observability.service_name = String::new();
    assert!(empty_service.validate().is_err());

    let mut unsafe_redaction = AiCoreConfig::default();
    unsafe_redaction.observability.redact_sensitive = false;
    let error = unsafe_redaction
        .validate()
        .expect_err("redaction cannot be disabled");
    assert!(error.to_string().contains("redact_sensitive"));
}

#[test]
fn parallelism_bounds_are_documented_and_enforced() {
    let mut config = AiCoreConfig::default();
    config.concurrency.max_concurrent_requests = MAX_CONCURRENT_REQUESTS;
    config.concurrency.max_blocking_tasks = MAX_BLOCKING_TASKS;
    assert!(config.validate().is_ok());
}

#[test]
fn configuration_round_trips_through_serde() {
    let mut config = AiCoreConfig::default();
    config.timeouts.request = Duration::from_secs(42);
    config.observability.level = LogLevel::Debug;

    let json = serde_json::to_string(&config).expect("config must serialize");
    let restored: AiCoreConfig = serde_json::from_str(&json).expect("config must deserialize");
    assert_eq!(config, restored);
}

#[test]
fn unknown_configuration_keys_are_rejected() {
    let json = r#"{"timeouts":{"request":5},"unknown_section":true}"#;
    assert!(serde_json::from_str::<AiCoreConfig>(json).is_err());
}

#[test]
fn log_level_parsing_is_case_insensitive() {
    assert_eq!(LogLevel::from_str("DEBUG").expect("valid"), LogLevel::Debug);
    assert_eq!(
        LogLevel::from_str("warning").expect("valid"),
        LogLevel::Warn
    );
    assert!(LogLevel::from_str("verbose").is_err());
    assert_eq!(LogLevel::Info.to_string(), "info");
}

#[test]
fn snapshots_are_immutable_and_versions_increase() {
    let store = ConfigStore::new(AiCoreConfig::default()).expect("default config is valid");
    let first: Arc<_> = store.snapshot();
    assert_eq!(first.version(), ConfigVersion::INITIAL);
    assert_eq!(store.version(), ConfigVersion::INITIAL);

    let mut updated = AiCoreConfig::default();
    updated.timeouts.request = Duration::from_millis(250);
    let second = store.apply(updated).expect("valid update");

    assert_eq!(second.version(), ConfigVersion::new(2));
    assert_eq!(first.version(), ConfigVersion::INITIAL);
    assert_eq!(
        first.config().timeouts.request,
        Duration::from_secs(60),
        "the old snapshot must not observe the update"
    );
    assert_eq!(second.config().timeouts.request, Duration::from_millis(250));
    assert_eq!(store.config().timeouts.request, Duration::from_millis(250));

    // Both handles point to distinct allocations.
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(Arc::strong_count(&first), 1);
}

#[test]
fn rejected_updates_leave_the_store_untouched() {
    let store = ConfigStore::new(AiCoreConfig::default()).expect("default config is valid");
    let mut invalid = AiCoreConfig::default();
    invalid.timeouts.streaming_idle = Duration::ZERO;

    let error = store.apply(invalid).expect_err("invalid config must fail");
    assert!(matches!(error, CoreError::Config { .. }));
    assert_eq!(store.version(), ConfigVersion::INITIAL);
    assert_eq!(
        store.config().timeouts.streaming_idle,
        Duration::from_secs(30)
    );
}

#[test]
fn snapshot_carries_creation_time_and_shared_config() {
    let store = ConfigStore::new(AiCoreConfig::default()).expect("valid");
    let snapshot = store.snapshot();
    assert!(snapshot.age() < Duration::from_secs(5));
    assert_eq!(
        snapshot.shared_config().timeouts.request,
        Duration::from_secs(60)
    );
    assert_eq!(
        snapshot
            .created_at()
            .elapsed()
            .map(|d| d.as_secs())
            .unwrap_or(0),
        0
    );
}

#[test]
fn config_version_display_and_parse() {
    let version = ConfigVersion::new(12);
    assert_eq!(version.to_string(), "v12");
    assert_eq!(ConfigVersion::from_str("v12").expect("valid"), version);
    assert_eq!(ConfigVersion::from_str("12").expect("valid"), version);
    assert!(ConfigVersion::from_str("version").is_err());
}

#[test]
fn temp_storage_configuration_is_isolated() {
    let first = common::TempStorage::new();
    let second = common::TempStorage::new();

    assert_ne!(first.root(), second.root());
    assert!(first.config().validate().is_ok());
    assert!(second.config().validate().is_ok());
    assert!(first
        .config()
        .storage_layout()
        .expect("valid")
        .root()
        .starts_with(first.root()));
}
