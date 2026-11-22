//! SAGE Intelligence 适配层（Library 集成）。
//!
//! 边界：
//!   - 本模块是客户端唯一接触 `ai-core` / `ai-core-tauri` 的地方；
//!   - 前端只能通过 `ai_*` 命令与 `ai-core://*` 事件访问 AI；
//!   - AI 拥有独立的存储目录（`app_data_dir/ai-core-data/`），
//!     **不与业务 SQLite（`sage.db`）共享任何连接或文件**；
//!   - API Key 由 [`secrets`] 管理，永不进入业务库、永不回传前端。

pub mod commands;
pub mod config;
pub mod emitter;
pub mod secrets;
pub mod state;
pub mod version;

pub use state::AiState;
