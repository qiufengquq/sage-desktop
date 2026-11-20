//! 知识库命令：全部为真实 SQLite 查询。

use serde_json::Value;
use tauri::AppHandle;

use crate::commands::{blocking, db};
use crate::error::AppResult;
use crate::models::knowledge::{KnowledgeFilter, KnowledgeGraph, KnowledgeSummary};
use crate::services::knowledge_service;

#[tauri::command]
pub async fn knowledge_graph(app: AppHandle) -> AppResult<KnowledgeGraph> {
    let db = db(&app);
    blocking(move || db.with_conn(|conn| knowledge_service::graph(conn))).await
}

#[tauri::command]
pub async fn knowledge_list(
    app: AppHandle,
    filter: Option<Value>,
) -> AppResult<Vec<KnowledgeSummary>> {
    let parsed = KnowledgeFilter::from_json(filter.as_ref());
    let db = db(&app);
    blocking(move || db.with_conn(|conn| knowledge_service::list(conn, &parsed))).await
}

#[tauri::command]
pub async fn knowledge_detail(app: AppHandle, id: String) -> AppResult<Value> {
    let db = db(&app);
    blocking(move || db.with_conn(|conn| knowledge_service::detail(conn, &id))).await
}

#[tauri::command]
pub async fn knowledge_neighbors(app: AppHandle, id: String) -> AppResult<Vec<KnowledgeSummary>> {
    let db = db(&app);
    blocking(move || db.with_conn(|conn| knowledge_service::neighbors(conn, &id))).await
}
