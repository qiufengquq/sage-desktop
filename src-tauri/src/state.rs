//! 应用状态：只持有数据库句柄。
//!
//! 第一阶段这里是内存 KV，第二阶段换成真实 SQLite；命令层拿到的永远是
//! `Arc<Database>`，因此可以安全地移入 `spawn_blocking` 闭包。

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::database::{Database, DB_FILE_NAME};
use crate::error::{AppError, AppResult};
use crate::repositories::{state_repository, user_repository};

pub struct AppState {
    pub db: Arc<Database>,
}

impl AppState {
    pub fn new(db: Database) -> Self {
        Self { db: Arc::new(db) }
    }

    /// 启动时初始化：定位文件 → 建库/迁移/内容同步 → 首启种子
    pub fn init(app: &AppHandle) -> AppResult<Self> {
        let path = resolve_db_path(app)?;
        if path
            .to_string_lossy()
            .to_lowercase()
            .contains("onedrive")
        {
            eprintln!(
                "[sage] 警告：数据库位于 OneDrive 目录（{}）。WAL 在同步盘上不可靠，建议更换数据目录。",
                path.display()
            );
        }

        let db = Database::open(&path)?;

        // 首启种子：默认用户 / 资料 / 设置 + 每个知识实体的状态行
        db.with_tx(|tx| {
            user_repository::ensure_seeded(tx)?;
            state_repository::ensure_rows_for_all_entities(tx)?;
            Ok(())
        })?;

        Ok(Self { db: Arc::new(db) })
    }

    pub fn db(&self) -> Arc<Database> {
        Arc::clone(&self.db)
    }
}

/// 数据文件位置：默认在应用数据目录，开发期可用 `SAGE_DB_PATH` 覆盖
fn resolve_db_path(app: &AppHandle) -> AppResult<PathBuf> {
    if let Ok(custom) = std::env::var("SAGE_DB_PATH") {
        if !custom.trim().is_empty() {
            return Ok(PathBuf::from(custom));
        }
    }

    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::internal(format!("无法定位应用数据目录：{e}")))?;

    Ok(dir.join(DB_FILE_NAME))
}
