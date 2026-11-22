//! 把 `ai-core-tauri` 的事件信封转发到 Tauri 前端。
//!
//! 事件名保持框架原样（`ai-core://...`），只在流结束时额外发一条客户端事件
//! `sage-ai://stream-finished`，用于把终态（文本/用量/中断原因）交给前端。

use ai_core_tauri::{EventEmitter, EventEnvelope};
use tauri::{AppHandle, Emitter};

/// 客户端自定义事件：一次流式请求的终态摘要
pub const FINISHED_EVENT: &str = "sage-ai://stream-finished";

pub struct TauriEmitter {
    app: AppHandle,
}

impl TauriEmitter {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    /// 发送终态摘要。失败只可能是前端窗口已销毁，忽略即可。
    pub fn finished(&self, payload: &serde_json::Value) {
        let _ = self.app.emit(FINISHED_EVENT, payload.clone());
    }
}

impl EventEmitter for TauriEmitter {
    fn emit(&self, envelope: EventEnvelope) {
        // 前端可能已经关闭或尚未订阅；事件丢失不应影响后端流本身
        let _ = self.app.emit(&envelope.event, envelope.payload);
    }
}
