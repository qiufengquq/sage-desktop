//! SQLite 数据库核心：连接、事务、迁移、备份、内容同步。
//!
//! 设计约束（见 docs/RUST_DATABASE_ARCHITECTURE.md）：
//! 1. 只有一个写连接，`Mutex` 串行化；不引入连接池（无实测依据前不加复杂度）。
//! 2. 所有触库调用都必须在 `spawn_blocking` 里执行，避免阻塞 UI 主线程。
//! 3. 写操作一律走 `with_tx`：`BEGIN IMMEDIATE` + 失败整体回滚。
//! 4. 仓储层只接收 `&Connection`，不认识 `Database`，便于在事务内复用。

pub mod backup;
pub mod content;
pub mod migrate;
pub mod pragmas;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::{AppError, AppResult};

/// 当前程序的数据库结构版本（与最后一个迁移文件一致）
pub const SCHEMA_VERSION: i64 = 3;

/// 数据库文件名
pub const DB_FILE_NAME: &str = "sage.db";

/// 统一时间格式：ISO8601 UTC，定长毫秒 + Z，字典序即时间序
pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub struct Database {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Database {
    /// 打开（必要时创建）数据库：建目录 → 配置 PRAGMA → 迁移 → 同步内容包
    pub fn open(path: impl AsRef<Path>) -> AppResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| AppError::internal(format!("无法创建数据目录：{e}")))?;
        }

        let mut conn = Connection::open(&path)?;
        pragmas::apply(&conn)?;
        migrate::run(&mut conn, &path)?;
        content::sync(&mut conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    /// 测试用：内存库（跑迁移，不含内容包）
    pub fn open_in_memory() -> AppResult<Self> {
        let mut conn = Connection::open_in_memory()?;
        pragmas::apply(&conn)?;
        migrate::run(&mut conn, Path::new(":memory:"))?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        })
    }

    /// 测试用：内存库 + 完整内容包（与生产启动后的状态一致，且不落盘、不抢锁）
    pub fn open_in_memory_with_content() -> AppResult<Self> {
        let mut conn = Connection::open_in_memory()?;
        pragmas::apply(&conn)?;
        migrate::run(&mut conn, Path::new(":memory:"))?;
        content::sync(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| AppError::internal("数据库连接状态异常（互斥锁已中毒），请重启应用"))
    }

    /// 只读查询
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
        let guard = self.lock()?;
        f(&guard)
    }

    /// 写操作：`BEGIN IMMEDIATE`，闭包返回 Err 则整体回滚并递增 db_revision
    pub fn with_tx<T>(&self, f: impl FnOnce(&Transaction<'_>) -> AppResult<T>) -> AppResult<T> {
        let mut guard = self.lock()?;
        let tx = guard.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match f(&tx) {
            Ok(value) => {
                bump_revision(&tx)?;
                tx.commit()?;
                Ok(value)
            }
            Err(err) => {
                // drop 时也会回滚，这里显式调用以便错误路径可预期
                let _ = tx.rollback();
                Err(err)
            }
        }
    }

    /// 主库 + WAL 文件的实际占用（前端"本地数据"页展示用）
    pub fn size_kb(&self) -> u64 {
        let mut total = 0u64;
        for suffix in ["", "-wal"] {
            let candidate = PathBuf::from(format!("{}{}", self.path.display(), suffix));
            if let Ok(meta) = fs::metadata(&candidate) {
                total += meta.len();
            }
        }
        // 至少显示 1KB，避免出现"0 KB 但有数据"的观感
        (total / 1024).max(1)
    }
}

/// 全局修订号：每次写事务 +1，前端据此触发重新查询
pub fn bump_revision(conn: &Connection) -> AppResult<i64> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO app_metadata(key, value, updated_at) VALUES ('db_revision', '1', ?1)
         ON CONFLICT(key) DO UPDATE SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT),
                                       updated_at = excluded.updated_at",
        [&now],
    )?;
    read_revision(conn)
}

pub fn read_revision(conn: &Connection) -> AppResult<i64> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM app_metadata WHERE key = 'db_revision'",
            [],
            |row| row.get(0),
        )
        .ok();
    Ok(raw.and_then(|v| v.parse::<i64>().ok()).unwrap_or(0))
}

pub fn metadata_get(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM app_metadata WHERE key = ?1")?;
    let mut rows = stmt.query([key])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

pub fn metadata_set(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO app_metadata(key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![key, value, now_iso()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
            .expect("count")
    }

    #[test]
    fn migration_creates_expected_shape() {
        let db = Database::open_in_memory().expect("open");
        db.with_conn(|conn| {
            let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            assert_eq!(version, SCHEMA_VERSION);

            let tables: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )?;
            // 36 张业务表 + schema_migrations（0003 新增 ai_providers）
            assert_eq!(tables, 37, "表数量与设计不符");

            let views: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'view'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(views, 1, "knowledge_index 视图缺失");
            Ok(())
        })
        .expect("schema");
    }

    #[test]
    fn migration_is_idempotent() {
        let path = std::env::temp_dir().join(format!("sage-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let first = Database::open(&path).expect("first open");
        first.with_conn(|conn| {
            assert_eq!(count(conn, "schema_migrations"), SCHEMA_VERSION);
            Ok(())
        }).expect("first");
        drop(first);

        let second = Database::open(&path).expect("second open");
        second.with_conn(|conn| {
            assert_eq!(count(conn, "schema_migrations"), SCHEMA_VERSION, "重复迁移");
            Ok(())
        }).expect("second");

        drop(second);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn transaction_rolls_back_on_error() {
        let db = Database::open_in_memory().expect("open");
        db.with_conn(|conn| {
            metadata_set(conn, "tx_probe", "before")?;
            Ok(())
        }).expect("seed");

        let outcome: AppResult<()> = db.with_tx(|tx| {
            metadata_set(tx, "tx_probe", "after")?;
            Err(AppError::internal("模拟失败"))
        });
        assert!(outcome.is_err(), "闭包返回 Err 必须整体回滚");

        db.with_conn(|conn| {
            let value = metadata_get(conn, "tx_probe")?.expect("exists");
            assert_eq!(value, "before", "回滚后不应残留写入");
            assert_eq!(read_revision(conn)?, 0, "回滚不应递增修订号");
            Ok(())
        }).expect("verify");
    }

    #[test]
    fn revision_increments_per_transaction() {
        let db = Database::open_in_memory().expect("open");
        for expected in 1..=3 {
            db.with_tx(|tx| {
                metadata_set(tx, "probe", "x")?;
                Ok(())
            }).expect("tx");
            db.with_conn(|conn| {
                assert_eq!(read_revision(conn)?, expected);
                Ok(())
            }).expect("rev");
        }
    }
}
