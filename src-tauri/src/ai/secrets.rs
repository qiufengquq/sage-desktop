//! API Key 的本地存放。
//!
//! 为什么不用业务 SQLite：
//!   1. 业务库会随着"本地数据"功能被用户导出/清理，密钥不应跟着走；
//!   2. `ai-repository` 的表会被 `local_data_summary` 统计，密钥表不该出现在学习数据里；
//!   3. 独立文件便于将来替换为 OS 凭据库而不动数据模型。
//!
//! 安全边界：
//!   - 文件只由 Rust 读写，`AiProviderView` 只暴露 `hasApiKey`；
//!   - 不写日志、不进错误消息、不参与 `serde` 序列化输出；
//!   - 已知限制：未接入 Windows Credential Manager（见 RELEASE_NOTES_1.0.1.md）。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

const DIR_NAME: &str = "ai";
const FILE_NAME: &str = "provider-keys.json";

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(DIR_NAME).join(FILE_NAME)
}

/// 读取全部密钥（providerId → key）。文件不存在视为"尚未配置"。
pub fn load(data_dir: &Path) -> AppResult<BTreeMap<String, String>> {
    let path = path(data_dir);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let raw = fs::read_to_string(&path)
        .map_err(|err| AppError::internal(format!("读取 AI 凭据文件失败：{err}")))?;
    let keys = serde_json::from_str::<BTreeMap<String, String>>(&raw)
        .map_err(|_| AppError::internal("AI 凭据文件格式不合法，请删除后重新配置"))?;
    Ok(keys)
}

/// 写入全部密钥。调用方负责决定是"新增/替换"还是"删除"。
pub fn save(data_dir: &Path, keys: &BTreeMap<String, String>) -> AppResult<()> {
    let path = path(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| AppError::internal(format!("创建 AI 配置目录失败：{err}")))?;
    }

    let raw = serde_json::to_string_pretty(keys)
        .map_err(|err| AppError::internal(format!("序列化 AI 凭据失败：{err}")))?;
    fs::write(&path, raw)
        .map_err(|err| AppError::internal(format!("写入 AI 凭据文件失败：{err}")))?;
    Ok(())
}

/// `Some(non_empty)` 写入、`Some(empty)` 删除、`None` 保持不变。
pub fn apply(data_dir: &Path, provider_id: &str, api_key: Option<&str>) -> AppResult<bool> {
    let mut keys = load(data_dir)?;
    let has_key = match api_key {
        Some(value) if !value.trim().is_empty() => {
            keys.insert(provider_id.to_string(), value.trim().to_string());
            true
        }
        Some(_) => {
            keys.remove(provider_id);
            false
        }
        None => keys.contains_key(provider_id),
    };
    save(data_dir, &keys)?;
    Ok(has_key)
}

/// 删除某个 Provider 的密钥（删除 Provider 时调用）。
pub fn remove(data_dir: &Path, provider_id: &str) -> AppResult<()> {
    let mut keys = load(data_dir)?;
    if keys.remove(provider_id).is_some() {
        save(data_dir, &keys)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_file_means_no_keys() {
        let dir = std::env::temp_dir().join(format!("sage-ai-secrets-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(load(&dir).expect("load").is_empty());
    }

    #[test]
    fn apply_writes_updates_and_removes() {
        let dir = std::env::temp_dir().join(format!("sage-ai-secrets-apply-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        assert!(apply(&dir, "deepseek", Some("sk-test")).expect("insert"));
        assert_eq!(load(&dir).expect("load").get("deepseek").map(String::as_str), Some("sk-test"));

        // 空串表示删除
        assert!(!apply(&dir, "deepseek", Some("   ")).expect("clear"));
        assert!(load(&dir).expect("load").is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_none_keeps_existing() {
        let dir = std::env::temp_dir().join(format!("sage-ai-secrets-keep-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        apply(&dir, "p", Some("k")).expect("insert");
        assert!(apply(&dir, "p", None).expect("keep"));

        let _ = fs::remove_dir_all(&dir);
    }
}
