//! 复习日志（`review_logs`）。
//!
//! 只追加、不修改，与 `mastery_events` 分工不同：
//! 这里回答"这次复习把条目排到了什么时候、发了多少 XP"，
//! 掌握度账本只回答"分数怎么变的"。

use rusqlite::Connection;

use crate::error::AppResult;

pub struct ReviewLogInsert<'a> {
    pub id: &'a str,
    /// 客户端幂等键，可空；非空时由唯一索引兜底
    pub submission_id: Option<&'a str>,
    pub entity_id: &'a str,
    pub rating: &'a str,
    pub prev_due_at: Option<&'a str>,
    pub next_due_at: &'a str,
    pub prev_interval_days: i64,
    pub next_interval_days: i64,
    pub ease: f64,
    pub mastery_before: f64,
    pub mastery_after: f64,
    pub xp: i64,
    pub at: &'a str,
}

/// 幂等回放所需的既有日志
#[derive(Debug, Clone)]
pub struct StoredReviewLog {
    pub entity_id: String,
    pub rating: String,
    pub next_due_at: String,
    pub next_interval_days: i64,
    pub ease: f64,
    pub mastery_before: f64,
    pub mastery_after: f64,
    pub xp: i64,
}

pub fn insert(conn: &Connection, log: &ReviewLogInsert<'_>) -> AppResult<()> {
    conn.execute(
        "INSERT INTO review_logs
           (id, submission_id, entity_id, rating, prev_due_at, next_due_at,
            prev_interval_days, next_interval_days, ease,
            mastery_before, mastery_after, xp, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        rusqlite::params![
            log.id,
            log.submission_id,
            log.entity_id,
            log.rating,
            log.prev_due_at,
            log.next_due_at,
            log.prev_interval_days,
            log.next_interval_days,
            log.ease,
            log.mastery_before,
            log.mastery_after,
            log.xp,
            log.at,
        ],
    )?;
    Ok(())
}

/// 幂等键查询：同一 `submissionId` 只允许写一行
pub fn find_by_submission(conn: &Connection, submission_id: &str) -> AppResult<Option<StoredReviewLog>> {
    let mut stmt = conn.prepare(
        "SELECT entity_id, rating, next_due_at, next_interval_days, ease,
                mastery_before, mastery_after, xp
           FROM review_logs WHERE submission_id = ?1",
    )?;
    let mut rows = stmt.query([submission_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(StoredReviewLog {
            entity_id: row.get(0)?,
            rating: row.get(1)?,
            next_due_at: row.get(2)?,
            next_interval_days: row.get(3)?,
            ease: row.get(4)?,
            mastery_before: row.get(5)?,
            mastery_after: row.get(6)?,
            xp: row.get(7)?,
        })),
        None => Ok(None),
    }
}

pub fn count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM review_logs", [], |row| row.get(0))?)
}

/// 复习获得的 XP 总量（等级由 `training_sessions.xp + review_logs.xp` 派生）
pub fn total_xp(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COALESCE(SUM(xp), 0) FROM review_logs", [], |row| row.get(0))?)
}
