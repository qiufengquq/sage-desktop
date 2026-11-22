//! AI 运行状态：配置快照、输出引擎、流式请求句柄。
//!
//! 设计约束：
//!   1. 不新建数据库连接管理器 —— 需要读配置时由调用方传入业务库连接；
//!   2. AI 初始化失败**不会**让应用启动失败（AI 是可选能力）；
//!   3. 所有跨线程共享的类型都是 `Send + Sync`；
//!   4. 锁顺序固定为 `inner` → `streams`，避免死锁。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use ai_core::output::OutputEngine;
use ai_core::provider::ProviderRuntime;
use ai_core::{AiCoreConfig, ConfigStore};
use ai_core_tauri::StreamCommand;
use rusqlite::Connection;

use crate::ai::{config, secrets, version};
use crate::database::now_iso;
use crate::error::{AppError, AppResult};
use crate::models::ai::{AiProviderRecord, AiProviderView, AiStatusView};
use crate::repositories::ai_repository;

/// 独立存储目录名：必须与业务库 `sage.db` 分开
const STORAGE_DIR: &str = "ai-core-data";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Uninitialized,
    Initializing,
    Ready,
    Error,
    Shutdown,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::Initializing => "initializing",
            Self::Ready => "ready",
            Self::Error => "error",
            Self::Shutdown => "shutdown",
        }
    }
}

struct Inner {
    status: Status,
    started_at: Option<String>,
    last_error: Option<String>,
    store: Option<Arc<ConfigStore>>,
    engine: Option<Arc<OutputEngine>>,
    providers: Vec<AiProviderView>,
    default_provider: Option<String>,
    default_model: Option<String>,
}

pub struct AiState {
    inner: Mutex<Inner>,
    streams: Mutex<BTreeMap<String, StreamCommand>>,
    data_dir: PathBuf,
}

impl AiState {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            inner: Mutex::new(Inner {
                status: Status::Uninitialized,
                started_at: None,
                last_error: None,
                store: None,
                engine: None,
                providers: Vec::new(),
                default_provider: None,
                default_model: None,
            }),
            streams: Mutex::new(BTreeMap::new()),
            data_dir,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn storage_root(&self) -> PathBuf {
        self.data_dir.join(STORAGE_DIR)
    }

    /// 启动时调用。失败只记录状态，不向上抛错。
    pub fn initialize(&self, conn: &Connection) {
        self.lock_inner().status = Status::Initializing;
        match self.install(conn) {
            Ok(()) => {
                let mut inner = self.lock_inner();
                inner.status = Status::Ready;
                inner.started_at = Some(now_iso());
                inner.last_error = None;
            }
            Err(err) => {
                let mut inner = self.lock_inner();
                inner.status = Status::Error;
                inner.last_error = Some(err.to_string());
            }
        }
    }

    /// Provider 变更后重建配置：已有 `ConfigStore` 走热更新，否则走完整初始化。
    pub fn reload(&self, conn: &Connection) -> AppResult<AiStatusView> {
        let existing = self.lock_inner().store.clone();
        match existing {
            Some(store) => {
                let records = ai_repository::load_all(conn)?;
                let keys = self.keys()?;
                let cfg = config::build(self.storage_root(), &records, &keys)?;
                store.apply(cfg).map_err(core_error)?;
                self.install_views(&records, &keys);
                let mut inner = self.lock_inner();
                inner.status = Status::Ready;
                inner.last_error = None;
                Ok(self.snapshot())
            }
            None => {
                self.install(conn)?;
                let mut inner = self.lock_inner();
                inner.status = Status::Ready;
                inner.started_at = Some(now_iso());
                inner.last_error = None;
                Ok(self.snapshot())
            }
        }
    }

    fn install(&self, conn: &Connection) -> AppResult<()> {
        let records = ai_repository::load_all(conn)?;
        let keys = self.keys()?;
        let cfg = config::build(self.storage_root(), &records, &keys)?;
        let store = Arc::new(ConfigStore::new(cfg).map_err(core_error)?);
        let runtime = Arc::new(ProviderRuntime::new(Arc::clone(&store)).map_err(core_error)?);
        let engine = Arc::new(OutputEngine::new(runtime));

        self.install_views(&records, &keys);
        let mut inner = self.lock_inner();
        inner.store = Some(store);
        inner.engine = Some(engine);
        Ok(())
    }

    fn install_views(&self, records: &[AiProviderRecord], keys: &HashMap<String, String>) {
        let views: Vec<AiProviderView> = records
            .iter()
            .map(|record| {
                let mut view = record.to_view();
                // 以凭据文件为准，避免库里的 has_api_key 缓存与实际不一致
                view.has_api_key = keys.contains_key(&record.id);
                view
            })
            .collect();

        let active = records.iter().find(|record| record.enabled);
        let default_provider = active.map(|record| record.id.clone());
        let default_model = active.and_then(|record| record.default_model.clone());

        let mut inner = self.lock_inner();
        inner.providers = views;
        inner.default_provider = default_provider;
        inner.default_model = default_model;
    }

    fn keys(&self) -> AppResult<HashMap<String, String>> {
        Ok(secrets::load(&self.data_dir)?.into_iter().collect())
    }

    /// 已就绪的输出引擎；未就绪时给出可执行的错误。
    pub fn engine(&self) -> AppResult<Arc<OutputEngine>> {
        self.lock_inner().engine.clone().ok_or_else(|| {
            AppError::new(
                "unavailable",
                "AI 服务未就绪：请先在「设置 → AI」中配置 Provider",
            )
        })
    }

    /// 配置快照（供调试与默认模型解析）。
    pub fn config(&self) -> Option<Arc<AiCoreConfig>> {
        self.lock_inner().store.as_ref().map(|store| store.config())
    }

    pub fn snapshot(&self) -> AiStatusView {
        let inner = self.lock_inner();
        AiStatusView {
            status: inner.status.as_str().to_string(),
            started_at: inner.started_at.clone(),
            last_error: inner.last_error.clone(),
            providers: inner.providers.clone(),
            default_provider: inner.default_provider.clone(),
            default_model: inner.default_model.clone(),
            active_streams: self.active_streams(),
            version: version::info(),
        }
    }

    /* ------------------------------ 流式句柄 ------------------------------ */

    pub fn register_stream(&self, request_id: &str, command: StreamCommand) {
        self.lock_streams().insert(request_id.to_string(), command);
    }

    pub fn take_stream(&self, request_id: &str) -> Option<StreamCommand> {
        self.lock_streams().remove(request_id)
    }

    /// 请求取消；返回是否命中在途请求。
    pub fn cancel_stream(&self, request_id: &str) -> bool {
        let mut streams = self.lock_streams();
        match streams.remove(request_id) {
            Some(command) => {
                command.cancel();
                true
            }
            None => false,
        }
    }

    pub fn active_streams(&self) -> usize {
        self.lock_streams().len()
    }

    /// 应用退出时标记状态（不强制中断，避免阻塞关闭流程）。
    pub fn shutdown(&self) {
        self.lock_inner().status = Status::Shutdown;
    }

    fn lock_inner(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_streams(&self) -> MutexGuard<'_, BTreeMap<String, StreamCommand>> {
        self.streams
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// 框架错误 → 客户端错误码。
///
/// 只映射到既有错误码集合，不新增前端不认识的 code；
/// 具体原因保留在 message 中（框架已对 message 做过脱敏）。
pub fn core_error(err: ai_core::CoreError) -> AppError {
    use ai_core::ErrorKind;

    let code = match err.kind() {
        ErrorKind::Authentication => "unauthorized",
        ErrorKind::InvalidArgument
        | ErrorKind::Config
        | ErrorKind::CapabilityUnsupported
        | ErrorKind::TokenLimitExceeded
        | ErrorKind::Serialization
        | ErrorKind::Tool => "invalid_argument",
        ErrorKind::Timeout | ErrorKind::Cancelled => "timeout",
        ErrorKind::NotFound => "not_found",
        _ => "unavailable",
    };
    AppError::new(code, err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sage-ai-state-{tag}-{}", std::process::id()))
    }

    #[test]
    fn fresh_state_is_uninitialized() {
        let state = AiState::new(temp_dir("fresh"));
        let view = state.snapshot();
        assert_eq!(view.status, "uninitialized");
        assert!(view.providers.is_empty());
        assert_eq!(view.version.protocol_version, version::PROTOCOL_VERSION);
        assert!(state.engine().is_err(), "未初始化时不得返回引擎");
    }

    #[test]
    fn initialize_without_providers_is_ready_but_idle() {
        let db = Database::open_in_memory().expect("open");
        let state = AiState::new(temp_dir("empty"));

        db.with_conn(|conn| {
            state.initialize(conn);
            Ok(())
        })
        .expect("init");

        let view = state.snapshot();
        assert_eq!(view.status, "ready", "没有 Provider 也应就绪，只是无法发请求");
        assert!(view.providers.is_empty());
        assert!(view.default_provider.is_none());
        assert!(state.engine().is_ok());
    }

    #[test]
    fn cancel_unknown_stream_is_false() {
        let state = AiState::new(temp_dir("cancel"));
        assert!(!state.cancel_stream("missing"));
        assert_eq!(state.active_streams(), 0);
    }
}
