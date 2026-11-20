//! 连接级 PRAGMA 配置。
//!
//! 关键点：`foreign_keys` 是**每连接**设置，不是持久化到文件的；每个新连接都必须设。
//! `journal_mode = WAL` 会写入文件头，设置一次后长期有效，但重复设置是幂等的。

use std::time::Duration;

use rusqlite::Connection;

use crate::error::AppResult;

/// 多实例 / 后台备份时的等待上限
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// page cache 上限（负数表示 KB）
const CACHE_SIZE_KB: i64 = -16_000;
/// WAL 自动检查点阈值（页）
const WAL_AUTOCHECKPOINT: i64 = 400;

pub fn apply(conn: &Connection) -> AppResult<()> {
    // journal_mode 是查询型 pragma，必须用 query_row 读取返回值
    let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;

    // WAL 下 NORMAL 已能保证"不会损坏数据库"，代价是崩溃可能丢最后一两个事务。
    // 取舍见 docs/RUST_DATABASE_ARCHITECTURE.md §3.3（Q8 已确认）
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "cache_size", CACHE_SIZE_KB)?;
    conn.pragma_update(None, "wal_autocheckpoint", WAL_AUTOCHECKPOINT)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;

    Ok(())
}

/// 体检：完整性 + 外键一致性。返回问题清单，空表示健康。
pub fn health_check(conn: &Connection) -> AppResult<Vec<String>> {
    let mut problems = Vec::new();

    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        problems.push(format!("完整性检查：{integrity}"));
    }

    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let table: String = row.get(0)?;
        let parent: String = row.get(2)?;
        problems.push(format!("外键不一致：{table} → {parent}"));
    }

    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn foreign_keys_are_enabled() {
        let db = Database::open_in_memory().expect("open");
        db.with_conn(|conn| {
            let enabled: i64 = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
            assert_eq!(enabled, 1);
            let mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
            // 内存库不支持 WAL，会回落 memory；文件库才是 wal
            assert!(mode == "wal" || mode == "memory", "unexpected journal_mode: {mode}");
            Ok(())
        })
        .expect("pragmas");
    }

    #[test]
    fn fresh_database_is_healthy() {
        let db = Database::open_in_memory().expect("open");
        let problems = db.with_conn(health_check).expect("health");
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn foreign_key_violation_is_rejected() {
        let db = Database::open_in_memory().expect("open");
        let result = db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO entity_states(entity_id, kind, updated_at) VALUES ('w_missing', 'word', '2026-01-01T00:00:00.000Z')",
                [],
            )?;
            Ok(())
        });
        assert!(result.is_err(), "外键必须阻止孤儿状态行");
    }
}
