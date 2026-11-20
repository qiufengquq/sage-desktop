//! 命令层：只做参数解析与调度，业务逻辑在 `services`，SQL 在 `repositories`。

pub mod app;
pub mod knowledge;
pub mod learning;
pub mod user;

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// 取数据库句柄（`Arc` 可安全移入阻塞线程）
pub(crate) fn db(app: &AppHandle) -> Arc<Database> {
    app.state::<AppState>().db()
}

/// 把同步的 SQLite 操作丢到阻塞线程池，避免占用 Tauri 主线程/异步运行时
pub(crate) async fn blocking<T, F>(task: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(task).await {
        Ok(result) => result,
        Err(err) => Err(AppError::internal(format!("数据库任务异常终止：{err}"))),
    }
}
