//! 掌握度变化账本（`mastery_events`）。
//!
//! 只追加、不修改。它与 `review_logs` 分工不同：
//! 这里回答"掌握度怎么变的"，对训练和复习都记账。

use rusqlite::Connection;

use crate::error::AppResult;

pub struct MasteryEventInsert<'a> {
    pub id: &'a str,
    pub entity_id: &'a str,
    pub kind: &'a str,
    pub mastery_before: f64,
    pub mastery_after: f64,
    pub source: &'a str,
    pub ref_id: &'a str,
    pub at: &'a str,
}

pub fn insert(conn: &Connection, event: &MasteryEventInsert<'_>) -> AppResult<()> {
    conn.execute(
        "INSERT INTO mastery_events
           (id, entity_id, kind, mastery_before, mastery_after, source, ref_id, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            event.id,
            event.entity_id,
            event.kind,
            event.mastery_before,
            event.mastery_after,
            event.source,
            event.ref_id,
            event.at,
        ],
    )?;
    Ok(())
}

pub fn count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM mastery_events", [], |row| row.get(0))?)
}
