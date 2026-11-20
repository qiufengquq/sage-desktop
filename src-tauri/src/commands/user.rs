//! 用户资料、设置与本地数据概览。
//!
//! 全部改为真实 SQLite 读写；命令体保持"取句柄 → 阻塞线程 → 调仓储"三步。

use serde_json::Value;
use tauri::AppHandle;

use crate::commands::{blocking, db};
use crate::error::{AppError, AppResult};
use crate::models::user::{AppSettings, LocalDataSummary, SyncStatus, UserProfile};
use crate::repositories::user_repository;

#[tauri::command]
pub async fn settings_get(app: AppHandle) -> AppResult<AppSettings> {
    let db = db(&app);
    blocking(move || db.with_conn(|conn| user_repository::settings_get(conn))).await
}

#[tauri::command]
pub async fn settings_update(app: AppHandle, patch: Value) -> AppResult<AppSettings> {
    let db = db(&app);
    blocking(move || db.with_tx(|tx| user_repository::settings_update(tx, &patch))).await
}

#[tauri::command]
pub async fn user_profile_get(app: AppHandle) -> AppResult<UserProfile> {
    let db = db(&app);
    blocking(move || db.with_conn(|conn| user_repository::profile_get(conn))).await
}

/// 资料更新。注意：修改每日预算/目标等级后重排学习计划属于阶段 C5（计划模块），
/// 当前只落库资料本身，不会假装计划已经重算。
#[tauri::command]
pub async fn user_profile_update(app: AppHandle, patch: Value) -> AppResult<UserProfile> {
    let db = db(&app);
    blocking(move || db.with_tx(|tx| user_repository::profile_update(tx, &patch))).await
}

#[tauri::command]
pub async fn sync_status_get(app: AppHandle) -> AppResult<SyncStatus> {
    let db = db(&app);
    blocking(move || db.with_conn(|conn| user_repository::sync_status(conn, db.size_kb()))).await
}

/// 云同步不在第二阶段范围：明确报"未提供"，不伪造已同步状态
#[tauri::command]
pub async fn sync_trigger() -> AppResult<SyncStatus> {
    Err(AppError::new(
        "unavailable",
        "云同步尚未提供，学习数据仅保存在本机。",
    ))
}

#[tauri::command]
pub async fn local_data_summary(app: AppHandle) -> AppResult<LocalDataSummary> {
    let db = db(&app);
    blocking(move || {
        db.with_conn(|conn| user_repository::local_data_summary(conn, db.size_kb()))
    })
    .await
}

/// 清理学习记录属于阶段 C7（需要连同计划重建一起做，避免留下悬空计划）。
#[tauri::command]
pub async fn local_data_clear() -> AppResult<Value> {
    Err(AppError::new(
        "unavailable",
        "数据清理将在学习计划模块接入后提供（阶段 C7）。",
    ))
}
