//! AI 相关 Tauri 命令。
//!
//! 约定（与现有 commands 一致）：
//!   - 触库操作一律 `blocking(db.with_tx/with_conn)`；
//!   - 返回既有错误码，不新增前端不认识的 code；
//!   - API Key 只进不出：输入可以带，输出永远是 `hasApiKey`。

use std::sync::atomic::{AtomicU64, Ordering};

use ai_core::output::OutputOutcome;
use ai_core::provider::config::{ModelSpec, ProviderId};
use ai_core::provider::types::{AiRequest, Message};
use ai_core::provider::CallContext;
use ai_core_tauri::StreamCommand;
use chrono::Utc;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::ai::emitter::TauriEmitter;
use crate::ai::state::{self, AiState};
use crate::ai::{secrets, version};
use crate::commands::{blocking, db};
use crate::database::now_iso;
use crate::error::{AppError, AppResult};
use crate::models::ai::{
    AiCompletionView, AiErrorView, AiProbeView, AiProviderInput, AiProviderRecord, AiProviderView,
    AiStatusView, AiStreamHandle, AiVersionInfo,
};
use crate::repositories::ai_repository;
use crate::state::AppState;

static STREAM_SEQ: AtomicU64 = AtomicU64::new(0);

/* ---------------------------------- 状态 ---------------------------------- */

#[tauri::command]
pub fn ai_status(app: AppHandle) -> AppResult<AiStatusView> {
    Ok(app.state::<AppState>().ai.snapshot())
}

#[tauri::command]
pub fn ai_version() -> AiVersionInfo {
    version::info()
}

/* --------------------------------- Provider -------------------------------- */

/// 新增或更新 Provider。`apiKey` 缺省表示沿用已有密钥，空串表示删除。
#[tauri::command]
pub async fn ai_provider_upsert(
    app: AppHandle,
    provider: AiProviderInput,
) -> AppResult<AiProviderView> {
    let db = db(&app);
    let ai = app.state::<AppState>().ai.clone();

    blocking(move || {
        db.with_tx(|tx| {
            let id = provider.id.trim().to_string();
            if id.is_empty() {
                return Err(AppError::invalid("providerId 不能为空"));
            }
            let base_url = provider.base_url.trim().trim_end_matches('/').to_string();
            if base_url.is_empty() {
                return Err(AppError::invalid("baseUrl 不能为空"));
            }

            let has_api_key = secrets::apply(ai.data_dir(), &id, provider.api_key.as_deref())?;

            let display_name = {
                let trimmed = provider.display_name.trim();
                if trimmed.is_empty() {
                    id.clone()
                } else {
                    trimmed.to_string()
                }
            };
            let models: Vec<String> = provider
                .models
                .unwrap_or_default()
                .into_iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect();

            let record = AiProviderRecord {
                id,
                display_name,
                protocol: normalize_protocol(&provider.protocol),
                base_url,
                enabled: provider.enabled.unwrap_or(true),
                default_model: provider
                    .default_model
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty()),
                models,
                has_api_key,
                updated_at: now_iso(),
            };

            ai_repository::upsert(tx, &record)?;
            ai.reload(tx)?;
            Ok(record.to_view())
        })
    })
    .await
}

#[tauri::command]
pub async fn ai_provider_delete(app: AppHandle, provider_id: String) -> AppResult<bool> {
    let db = db(&app);
    let ai = app.state::<AppState>().ai.clone();

    blocking(move || {
        db.with_tx(|tx| {
            let removed = ai_repository::delete(tx, &provider_id)? > 0;
            if removed {
                secrets::remove(ai.data_dir(), &provider_id)?;
                ai.reload(tx)?;
            }
            Ok(removed)
        })
    })
    .await
}

/// 连接探测：发一条极小请求验证"密钥 + 模型 + 网络"链路是否可用。
/// 这不是上游的专用 API，只是用一次真实调用做可达性判断。
#[tauri::command]
pub async fn ai_provider_test(
    app: AppHandle,
    provider_id: Option<String>,
) -> AppResult<AiProbeView> {
    let ai = app.state::<AppState>().ai.clone();
    let target = resolve_target(&ai, provider_id.as_deref(), None)?;
    let Some(target) = target else {
        return Err(AppError::new(
            "unavailable",
            "还没有可用的 AI Provider，请先在设置中配置",
        ));
    };
    let engine = ai.engine()?;

    let request = AiRequest::user_prompt("ping")
        .with_model(target.spec.clone())
        .with_max_output_tokens(16)
        .with_temperature(0.0);

    let started = std::time::Instant::now();
    let context = CallContext::generate();
    let outcome = engine.complete(request, &context).await;

    let latency_ms = started.elapsed().as_millis() as i64;
    match outcome {
        Ok(result) => Ok(AiProbeView {
            provider_id: target.provider_id,
            model: target.model,
            ok: !result.interrupted,
            latency_ms,
            message: result.error.map(|error| error.message),
        }),
        Err(error) => Ok(AiProbeView {
            provider_id: target.provider_id,
            model: target.model,
            ok: false,
            latency_ms,
            message: Some(state::core_error(error).to_string()),
        }),
    }
}

/* -------------------------------- 一次性请求 ------------------------------- */

#[tauri::command]
pub async fn ai_complete(
    app: AppHandle,
    prompt: String,
    provider_id: Option<String>,
    model: Option<String>,
    system: Option<String>,
) -> AppResult<AiCompletionView> {
    let ai = app.state::<AppState>().ai.clone();
    let target = resolve_target(&ai, provider_id.as_deref(), model.as_deref())?;
    let engine = ai.engine()?;
    let request = build_request(&prompt, system.as_deref(), target.as_ref())?;

    let context = CallContext::generate();
    let outcome = engine.complete(request, &context).await.map_err(state::core_error)?;

    Ok(completion_view(outcome, target.as_ref().map(|item| item.model.clone())))
}

/* ---------------------------------- 流式 ---------------------------------- */

#[tauri::command]
pub async fn ai_stream_start(
    app: AppHandle,
    prompt: String,
    provider_id: Option<String>,
    model: Option<String>,
    system: Option<String>,
) -> AppResult<AiStreamHandle> {
    let ai = app.state::<AppState>().ai.clone();
    let target = resolve_target(&ai, provider_id.as_deref(), model.as_deref())?;
    let engine = ai.engine()?;
    let request = build_request(&prompt, system.as_deref(), target.as_ref())?;

    let request_id = next_request_id();
    let command = StreamCommand::new(CallContext::generate());
    ai.register_stream(&request_id, command.clone());

    let emitter = TauriEmitter::new(app.clone());
    let task_state = ai.clone();
    let task_request_id = request_id.clone();

    tauri::async_runtime::spawn(async move {
        let result = ai_core_tauri::start_stream(&engine, request, &command, &emitter).await;
        // 无论成功/失败都回收句柄，避免 active_streams 泄漏
        task_state.take_stream(&task_request_id);

        let payload = match result {
            Ok(outcome) => finished_payload(&task_request_id, &outcome),
            Err(error) => json!({
                "requestId": task_request_id,
                "text": "",
                "reasoning": "",
                "interrupted": true,
                "finished": false,
                "error": { "code": "unavailable", "message": error.to_string(), "retryable": false },
            }),
        };
        emitter.finished(&payload);
    });

    Ok(AiStreamHandle {
        request_id,
        provider_id: target.as_ref().map(|item| item.provider_id.clone()).unwrap_or_default(),
        model: target
            .as_ref()
            .map(|item| item.model.clone())
            .unwrap_or_else(|| "default".to_string()),
    })
}

#[tauri::command]
pub fn ai_stream_cancel(app: AppHandle, request_id: String) -> AppResult<bool> {
    Ok(app.state::<AppState>().ai.cancel_stream(&request_id))
}

/* --------------------------------- 内部工具 -------------------------------- */

struct Target {
    provider_id: String,
    model: String,
    spec: ModelSpec,
}

/// 解析"用哪个 Provider 的哪个模型"：
///   - 显式 providerId + 显式 model → 直接用；
///   - 只给 providerId → 用它的 defaultModel；
///   - 都不给 → 用第一个启用的 Provider（与设置页展示的默认一致）；
///   - 一个都没有 → `None`，交给上游默认路由（或由调用方报 unavailable）。
fn resolve_target(
    ai: &AiState,
    provider_id: Option<&str>,
    model: Option<&str>,
) -> AppResult<Option<Target>> {
    let view = ai.snapshot();
    let provider = match provider_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(id) => view
            .providers
            .iter()
            .find(|item| item.id == id)
            .ok_or_else(|| AppError::not_found(format!("AI Provider {id}")))?,
        None => match view.providers.iter().find(|item| item.enabled) {
            Some(item) => item,
            None => return Ok(None),
        },
    };

    let model_id = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| provider.default_model.clone())
        .or_else(|| provider.models.first().cloned())
        .ok_or_else(|| AppError::invalid(format!("Provider {} 没有可用模型", provider.id)))?;

    let id = ProviderId::new(provider.id.clone())
        .map_err(|err| AppError::invalid(format!("Provider id 不合法：{err}")))?;
    let spec = ModelSpec::new(id, model_id.clone())
        .map_err(|err| AppError::invalid(format!("模型标识不合法：{err}")))?;

    Ok(Some(Target {
        provider_id: provider.id.clone(),
        model: model_id,
        spec,
    }))
}

fn build_request(prompt: &str, system: Option<&str>, target: Option<&Target>) -> AppResult<AiRequest> {
    let text = prompt.trim();
    if text.is_empty() {
        return Err(AppError::invalid("prompt 不能为空"));
    }

    let mut messages = Vec::new();
    if let Some(system) = system.map(str::trim).filter(|value| !value.is_empty()) {
        messages.push(Message::system(system));
    }
    messages.push(Message::user(text));

    let mut request = AiRequest::new(messages).with_max_output_tokens(1024);
    if let Some(target) = target {
        request = request.with_model(target.spec.clone());
    }
    Ok(request)
}

fn completion_view(outcome: OutputOutcome, model: Option<String>) -> AiCompletionView {
    AiCompletionView {
        text: outcome.text,
        reasoning: outcome.reasoning,
        interrupted: outcome.interrupted,
        error: outcome.error.map(|error| AiErrorView {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
        }),
        usage: outcome
            .usage
            .as_ref()
            .and_then(|usage| serde_json::to_value(usage).ok()),
        finish_reason: outcome
            .finish_reason
            .as_ref()
            .and_then(|reason| serde_json::to_value(reason).ok()),
        model,
    }
}

fn finished_payload(request_id: &str, outcome: &OutputOutcome) -> Value {
    json!({
        "requestId": request_id,
        "text": outcome.text,
        "reasoning": outcome.reasoning,
        "interrupted": outcome.interrupted,
        "finished": true,
        "usage": outcome.usage.as_ref().and_then(|usage| serde_json::to_value(usage).ok()),
        "finishReason": outcome.finish_reason.as_ref().and_then(|reason| serde_json::to_value(reason).ok()),
        "error": outcome.error.as_ref().map(|error| json!({
            "code": error.code,
            "message": error.message,
            "retryable": error.retryable,
        })),
    })
}

fn normalize_protocol(raw: &str) -> String {
    match raw.trim() {
        "anthropic" => "anthropic".to_string(),
        _ => "openai-compatible".to_string(),
    }
}

fn next_request_id() -> String {
    let seq = STREAM_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("ai_{}_{seq}", Utc::now().timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_is_normalized() {
        assert_eq!(normalize_protocol("anthropic"), "anthropic");
        assert_eq!(normalize_protocol("openai-compatible"), "openai-compatible");
        assert_eq!(normalize_protocol("whatever"), "openai-compatible");
    }

    #[test]
    fn request_ids_are_unique() {
        let first = next_request_id();
        let second = next_request_id();
        assert_ne!(first, second);
        assert!(first.starts_with("ai_"));
    }

    #[test]
    fn empty_prompt_is_rejected_before_routing() {
        let outcome = build_request("   ", None, None);
        assert!(outcome.is_err());
    }
}
