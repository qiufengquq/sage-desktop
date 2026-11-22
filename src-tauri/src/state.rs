//! 应用状态：数据库句柄 + AI 运行状态。
//!
//! 只有一个 `AppState`：AI 不新建连接管理器，需要读配置时由命令层把业务库连接传进去。
//! 命令层拿到的永远是 `Arc<...>`，因此可以安全地移入 `spawn_blocking` 闭包。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::ai::AiState;
use crate::database::{Database, DB_FILE_NAME};
use crate::error::{AppError, AppResult};
use crate::repositories::{state_repository, user_repository};

pub struct AppState {
    pub db: Arc<Database>,
    pub ai: Arc<AiState>,
}

impl AppState {
    /// 启动时初始化：定位文件 → 建库/迁移/内容同步 → 首启种子 → AI 状态
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

        let db = Arc::new(db);

        // AI 数据目录跟随数据库目录：使用 `SAGE_DB_PATH` 隔离数据时，AI 也一起隔离，
        // 不会把测试数据写进真实的应用数据目录。
        let ai_dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let ai = Arc::new(AiState::new(ai_dir));

        // AI 初始化失败不阻塞启动：AI 是可选能力，状态与原因记录在 `AiStatusView`
        db.with_conn(|conn| {
            ai.initialize(conn);
            Ok(())
        })?;

        Ok(Self { db, ai })
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
