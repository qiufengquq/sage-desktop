//! 学习态仓储：`entity_states` 的读写与到期查询。
//!
//! 这张表是"当前掌握度"的唯一来源。事件表只追加，状态表可覆盖，
//! 每次覆盖都会递增 `row_version`，便于排查"谁最后改的"。

use rusqlite::Connection;

use crate::database::now_iso;
use crate::error::AppResult;
use crate::models::state::EntityState;

const COLUMNS: &str = "entity_id, kind, mastery, strength, state, review_count, lapse_count, ease, \
                       repetition, interval_days, due_at, last_reviewed_at, updated_at";

pub fn load_all(conn: &Connection) -> AppResult<Vec<EntityState>> {
    let sql = format!("SELECT {COLUMNS} FROM entity_states ORDER BY entity_id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], EntityState::from_row)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn find(conn: &Connection, entity_id: &str) -> AppResult<Option<EntityState>> {
    let sql = format!("SELECT {COLUMNS} FROM entity_states WHERE entity_id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([entity_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(EntityState::from_row(row)?)),
        None => Ok(None),
    }
}

/// 写入（不存在则创建），并递增行版本
pub fn upsert(conn: &Connection, state: &EntityState) -> AppResult<()> {
    conn.execute(
        "INSERT INTO entity_states
           (entity_id, kind, mastery, strength, state, review_count, lapse_count,
            ease, repetition, interval_days, due_at, last_reviewed_at, row_version, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, ?13)
         ON CONFLICT(entity_id) DO UPDATE SET
           kind             = excluded.kind,
           mastery          = excluded.mastery,
           strength         = excluded.strength,
           state            = excluded.state,
           review_count     = excluded.review_count,
           lapse_count      = excluded.lapse_count,
           ease             = excluded.ease,
           repetition       = excluded.repetition,
           interval_days    = excluded.interval_days,
           due_at           = excluded.due_at,
           last_reviewed_at = excluded.last_reviewed_at,
           row_version      = entity_states.row_version + 1,
           updated_at       = excluded.updated_at",
        rusqlite::params![
            state.entity_id,
            state.kind,
            state.mastery,
            state.strength,
            state.state,
            state.review_count,
            state.lapse_count,
            state.ease,
            state.repetition,
            state.interval_days,
            state.due_at,
            state.last_reviewed_at,
            state.updated_at,
        ],
    )?;
    Ok(())
}

/// 到期待复习（按到期时间升序），供复习队列使用
pub fn due(conn: &Connection, now: &str, limit: i64) -> AppResult<Vec<EntityState>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM entity_states
          WHERE due_at IS NOT NULL AND due_at <= ?1
          ORDER BY due_at
          LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params![now, limit], EntityState::from_row)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn count_due(conn: &Connection, now: &str) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM entity_states WHERE due_at IS NOT NULL AND due_at <= ?1",
        [now],
        |row| row.get(0),
    )?)
}

/// 为内容库中的每个实体补齐状态行（幂等，不覆盖已有学习记录）
pub fn ensure_rows_for_all_entities(conn: &Connection) -> AppResult<usize> {
    let now = now_iso();
    let inserted = conn.execute(
        "INSERT INTO entity_states(entity_id, kind, updated_at)
         SELECT e.id, e.kind, ?1 FROM entities e
          WHERE NOT EXISTS (SELECT 1 FROM entity_states s WHERE s.entity_id = e.id)",
        [&now],
    )?;
    Ok(inserted)
}

/// 清理学习记录：状态重置为初值，实体本身保留
pub fn reset_all(conn: &Connection) -> AppResult<usize> {
    let now = now_iso();
    let updated = conn.execute(
        "UPDATE entity_states SET
           mastery = 0, strength = 0, state = 'new',
           review_count = 0, lapse_count = 0,
           ease = 2.5, repetition = 0, interval_days = 0,
           due_at = NULL, last_reviewed_at = NULL,
           row_version = row_version + 1, updated_at = ?1",
        [&now],
    )?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn seed_entity(conn: &Connection, id: &str) -> AppResult<()> {
        conn.execute(
            "INSERT INTO entities(id, kind, difficulty, level, created_at, updated_at)
             VALUES (?1, 'word', 3, 'B1', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            [id],
        )?;
        Ok(())
    }

    #[test]
    fn upsert_then_find_roundtrip() {
        let db = Database::open_in_memory().expect("open");
        db.with_tx(|tx| {
            seed_entity(tx, "w_probe")?;
            let mut state = EntityState::new("w_probe", "word", "2026-11-14T00:00:00.000Z");
            state.mastery = 42.5;
            state.due_at = Some("2026-11-20T00:00:00.000Z".to_string());
            upsert(tx, &state)?;

            let loaded = find(tx, "w_probe")?.expect("存在");
            assert_eq!(loaded.mastery, 42.5);
            assert_eq!(loaded.due_at.as_deref(), Some("2026-11-20T00:00:00.000Z"));
            Ok(())
        })
        .expect("roundtrip");
    }

    #[test]
    fn upsert_increments_row_version() {
        let db = Database::open_in_memory().expect("open");
        db.with_tx(|tx| {
            seed_entity(tx, "w_version")?;
            let state = EntityState::new("w_version", "word", "2026-11-14T00:00:00.000Z");
            upsert(tx, &state)?;
            upsert(tx, &state)?;

            let version: i64 = tx.query_row(
                "SELECT row_version FROM entity_states WHERE entity_id = 'w_version'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(version, 2, "两次写入应留下可追溯的行版本");
            Ok(())
        })
        .expect("version");
    }

    #[test]
    fn due_query_respects_time_and_order() {
        let db = Database::open_in_memory().expect("open");
        db.with_tx(|tx| {
            for id in ["w_late", "w_early", "w_future", "w_none"] {
                seed_entity(tx, id)?;
                let mut state = EntityState::new(id, "word", "2026-11-01T00:00:00.000Z");
                state.due_at = match id {
                    "w_late" => Some("2026-11-13T00:00:00.000Z".to_string()),
                    "w_early" => Some("2026-11-10T00:00:00.000Z".to_string()),
                    "w_future" => Some("2026-12-01T00:00:00.000Z".to_string()),
                    _ => None,
                };
                upsert(tx, &state)?;
            }

            let now = "2026-11-14T00:00:00.000Z";
            let items = due(tx, now, 10)?;
            let ids: Vec<&str> = items.iter().map(|s| s.entity_id.as_str()).collect();
            assert_eq!(ids, vec!["w_early", "w_late"], "只返回到期项且按到期时间升序");
            assert_eq!(count_due(tx, now)?, 2);
            Ok(())
        })
        .expect("due");
    }

    #[test]
    fn ensure_rows_only_fills_missing() {
        let db = Database::open_in_memory().expect("open");
        db.with_tx(|tx| {
            seed_entity(tx, "w_a")?;
            seed_entity(tx, "w_b")?;

            assert_eq!(ensure_rows_for_all_entities(tx)?, 2);
            assert_eq!(ensure_rows_for_all_entities(tx)?, 0, "第二次不应新增");

            let mut state = EntityState::new("w_a", "word", "2026-11-14T00:00:00.000Z");
            state.mastery = 80.0;
            upsert(tx, &state)?;

            ensure_rows_for_all_entities(tx)?;
            let loaded = find(tx, "w_a")?.expect("存在");
            assert_eq!(loaded.mastery, 80.0, "补齐状态行不能覆盖已有学习记录");
            Ok(())
        })
        .expect("ensure");
    }

    #[test]
    fn reset_all_keeps_entities_but_clears_progress() {
        let db = Database::open_in_memory().expect("open");
        db.with_tx(|tx| {
            seed_entity(tx, "w_reset")?;
            ensure_rows_for_all_entities(tx)?;
            let mut state = EntityState::new("w_reset", "word", "2026-11-14T00:00:00.000Z");
            state.mastery = 90.0;
            state.state = "mastered".to_string();
            state.review_count = 5;
            upsert(tx, &state)?;

            assert_eq!(reset_all(tx)?, 1);
            let loaded = find(tx, "w_reset")?.expect("实体仍在");
            assert_eq!(loaded.mastery, 0.0);
            assert_eq!(loaded.state, "new");
            assert_eq!(loaded.review_count, 0);
            assert_eq!(loaded.ease, 2.5);
            Ok(())
        })
        .expect("reset");
    }
}
