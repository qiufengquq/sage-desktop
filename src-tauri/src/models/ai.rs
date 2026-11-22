//! AI 集成的读写模型。对应 `src/types/ai.ts`。
//!
//! 原则：
//!   1. 前端拿到的永远是"视图"，不包含 API Key、不包含 prompt 原文之外的框架内部结构。
//!   2. 上游（SAGE Intelligence）的 `usage` / `finish_reason` 原样以 JSON 透传，
//!      客户端不复制其字段定义，避免上游演进时双份维护。

use serde::{Deserialize, Serialize};

/* ---------------------------------- 版本 ---------------------------------- */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiVersionInfo {
    /// 客户端版本（Cargo.toml）
    pub client_version: String,
    /// SAGE Intelligence 版本（build.rs 从 Cargo.lock 注入）
    pub intelligence_version: String,
    /// 锁定的 Git commit
    pub intelligence_commit: String,
    /// 客户端与框架之间的集成契约版本
    pub protocol_version: String,
    /// 集成模式，恒为 `"library"`：与客户端同进程内嵌，不存在 sidecar 模式
    pub integration_mode: String,
    /// 构建时间（build.rs 注入）
    pub build_time: String,
    /// 构建平台
    pub platform: String,
}

/* --------------------------------- Provider -------------------------------- */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderView {
    pub id: String,
    pub display_name: String,
    /// openai-compatible | anthropic
    pub protocol: String,
    pub base_url: String,
    pub enabled: bool,
    pub default_model: Option<String>,
    pub models: Vec<String>,
    /// 只告诉前端"有没有配置密钥"，密钥本身永不出后端
    pub has_api_key: bool,
    pub updated_at: String,
}

/// 仓储层读取的记录（含明文密钥仅在内存中短暂存在）
#[derive(Debug, Clone)]
pub struct AiProviderRecord {
    pub id: String,
    pub display_name: String,
    pub protocol: String,
    pub base_url: String,
    pub enabled: bool,
    pub default_model: Option<String>,
    pub models: Vec<String>,
    pub has_api_key: bool,
    pub updated_at: String,
}

impl AiProviderRecord {
    pub fn to_view(&self) -> AiProviderView {
        AiProviderView {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            protocol: self.protocol.clone(),
            base_url: self.base_url.clone(),
            enabled: self.enabled,
            default_model: self.default_model.clone(),
            models: self.models.clone(),
            has_api_key: self.has_api_key,
            updated_at: self.updated_at.clone(),
        }
    }
}

/* ---------------------------------- 输入 ---------------------------------- */

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderInput {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    /// openai-compatible | anthropic，其它值按 openai-compatible 处理
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub base_url: String,
    pub default_model: Option<String>,
    #[serde(default)]
    pub models: Option<Vec<String>>,
    pub enabled: Option<bool>,
    /// 缺省 / null = 不修改已有密钥；空串 = 删除密钥
    pub api_key: Option<String>,
}

/* ---------------------------------- 状态 ---------------------------------- */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatusView {
    /// uninitialized | initializing | ready | error | shutdown
    pub status: String,
    pub started_at: Option<String>,
    pub last_error: Option<String>,
    pub providers: Vec<AiProviderView>,
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    /// 正在进行的流式请求数
    pub active_streams: usize,
    pub version: AiVersionInfo,
}

/* ---------------------------------- 结果 ---------------------------------- */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiErrorView {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCompletionView {
    pub text: String,
    pub reasoning: String,
    pub interrupted: bool,
    pub error: Option<AiErrorView>,
    /// 上游 usage 原样透传（不同 Provider 字段不同，客户端不做字段建模）
    pub usage: Option<serde_json::Value>,
    /// 上游 finish_reason 原样透传
    pub finish_reason: Option<serde_json::Value>,
    pub model: Option<String>,
}

/* --------------------------------- 探测 / 流 -------------------------------- */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProbeView {
    pub provider_id: String,
    pub model: String,
    /// 请求是否成功路由并返回（不代表内容质量）
    pub ok: bool,
    pub latency_ms: i64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStreamHandle {
    pub request_id: String,
    pub provider_id: String,
    pub model: String,
}
