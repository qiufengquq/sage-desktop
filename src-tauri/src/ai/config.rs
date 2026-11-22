//! 把数据库中的 Provider 记录 + 凭据组装成 `ai-core` 的配置。
//!
//! 这里只做"翻译"，不做业务判断：
//!   - 启用的 Provider 才进入配置；
//!   - 没有模型的 Provider 用 default_model 兜底；
//!   - default_model 必须存在于 models 列表，否则上游校验会拒绝。

use std::collections::HashMap;
use std::path::PathBuf;

use ai_core::config::{AiCoreConfig, StorageConfig};
use ai_core::provider::config::{ModelConfig, ProviderConfig, ProviderId, ProviderProtocol};

use crate::error::{AppError, AppResult};
use crate::models::ai::AiProviderRecord;

pub fn build(
    storage_root: PathBuf,
    providers: &[AiProviderRecord],
    keys: &HashMap<String, String>,
) -> AppResult<AiCoreConfig> {
    let mut config = AiCoreConfig::default();
    // AI 的 memory/metadata/cache 与业务库物理隔离（ai-core 强制独立目录）
    config.storage = StorageConfig { root: storage_root };

    for record in providers.iter().filter(|item| item.enabled) {
        // 上游要求"启用的 Provider 必须带密钥"。用户可能先存配置后填密钥，
        // 这种情况不进入运行时配置，但记录仍保留在库里，设置页会提示"待填密钥"。
        let Some(api_key) = keys
            .get(&record.id)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        let id = ProviderId::new(record.id.clone())
            .map_err(|err| AppError::invalid(format!("Provider id 不合法：{err}")))?;
        let protocol = protocol_of(&record.protocol);

        let mut models: Vec<String> = record.models.clone();
        if models.is_empty() {
            if let Some(model) = &record.default_model {
                models.push(model.clone());
            }
        }
        let default_model = record.default_model.clone().or_else(|| models.first().cloned());
        if let Some(model) = &default_model {
            if !models.contains(model) {
                models.push(model.clone());
            }
        }
        if models.is_empty() {
            return Err(AppError::invalid(format!(
                "Provider {} 没有可用模型，请先填写默认模型",
                record.id
            )));
        }

        let mut provider = ProviderConfig::new(id, protocol).with_base_url(record.base_url.clone());
        for model in &models {
            provider = provider.with_model(ModelConfig::new(model.clone()));
        }
        if let Some(model) = default_model {
            provider = provider.with_default_model(model);
        }
        provider = provider.with_api_key(api_key.to_string());

        config.providers.insert(provider.id.clone(), provider);
    }

    config
        .validate()
        .map_err(|err| AppError::invalid(format!("AI 配置校验未通过：{err}")))?;
    Ok(config)
}

fn protocol_of(raw: &str) -> ProviderProtocol {
    match raw {
        "anthropic" => ProviderProtocol::Anthropic,
        _ => ProviderProtocol::OpenAiCompatible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, models: Vec<&str>, default: Option<&str>) -> AiProviderRecord {
        AiProviderRecord {
            id: id.to_string(),
            display_name: id.to_string(),
            protocol: "openai-compatible".to_string(),
            base_url: "https://api.example.com".to_string(),
            enabled: true,
            default_model: default.map(str::to_string),
            models: models.into_iter().map(str::to_string).collect(),
            has_api_key: false,
            updated_at: "2026-11-22T00:00:00.000Z".to_string(),
        }
    }

    #[test]
    fn enabled_provider_reaches_config() {
        let mut keys = HashMap::new();
        keys.insert("p1".to_string(), "sk-test".to_string());
        let config = build(
            PathBuf::from("/tmp/ai"),
            &[record("p1", vec!["m1"], Some("m1"))],
            &keys,
        )
        .expect("build");

        let provider = config
            .providers
            .get(&ProviderId::new("p1").expect("id"))
            .expect("provider 存在");
        assert_eq!(provider.default_model.as_deref(), Some("m1"));
        assert!(provider.api_key.is_some(), "密钥应传入 ProviderConfig");
    }

    #[test]
    fn default_model_is_added_to_models() {
        let mut keys = HashMap::new();
        keys.insert("p1".to_string(), "sk-test".to_string());
        let config = build(
            PathBuf::from("/tmp/ai"),
            &[record("p1", vec!["m1"], Some("m2"))],
            &keys,
        )
        .expect("build");

        let provider = config.providers.values().next().expect("provider");
        assert!(provider.model("m2").is_some(), "default_model 必须出现在 models 中");
    }

    #[test]
    fn provider_without_models_is_rejected() {
        let mut keys = HashMap::new();
        keys.insert("p1".to_string(), "sk-test".to_string());
        let outcome = build(
            PathBuf::from("/tmp/ai"),
            &[record("p1", vec![], None)],
            &keys,
        );
        assert!(outcome.is_err(), "没有模型的 Provider 不应静默通过");
    }

    #[test]
    fn provider_without_key_is_skipped_not_fatal() {
        // 先存配置、后填密钥是正常流程：保存不能失败
        let config = build(
            PathBuf::from("/tmp/ai"),
            &[record("p1", vec!["m1"], Some("m1"))],
            &HashMap::new(),
        )
        .expect("缺少密钥不应导致整份配置校验失败");
        assert!(config.providers.is_empty(), "无密钥的 Provider 不进入运行时路由");
    }

    #[test]
    fn disabled_provider_is_skipped() {
        let mut disabled = record("p1", vec!["m1"], Some("m1"));
        disabled.enabled = false;
        let config = build(PathBuf::from("/tmp/ai"), &[disabled], &HashMap::new()).expect("build");
        assert!(config.providers.is_empty());
    }
}
