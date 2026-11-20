//! 用户资料与设置。
//!
//! 设置以"点分键"存储（`appearance.theme`、`reminder.time`…），
//! 这样 `settings_update` 的部分更新在 SQL 层就是 UPSERT，不需要读写整块 JSON。

use rusqlite::Connection;
use serde_json::{Map, Value as Json};

use crate::database::now_iso;
use crate::error::{AppError, AppResult};
use crate::models::user::{
    AppSettings, LocalDataSummary, SyncStatus, UserProfile, LOCAL_USER_ID,
};

/// 首次启动写入默认用户 / 资料 / 设置（幂等）
pub fn ensure_seeded(conn: &Connection) -> AppResult<()> {
    let now = now_iso();
    let user_count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
    if user_count == 0 {
        let profile = UserProfile::default_for(LOCAL_USER_ID, &now);
        conn.execute(
            "INSERT INTO users(id, handle, display_name, joined_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4, ?4)",
            rusqlite::params![profile.id, profile.handle, profile.display_name, profile.joined_at],
        )?;
        write_profile(conn, &profile, &now)?;
    }

    let settings_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM app_settings", [], |row| row.get(0))?;
    if settings_count == 0 {
        write_settings(conn, &AppSettings::default())?;
    }

    Ok(())
}

/* --------------------------------- 资料 ---------------------------------- */

pub fn profile_get(conn: &Connection) -> AppResult<UserProfile> {
    let mut stmt = conn.prepare(
        "SELECT u.id, u.display_name, u.handle, u.joined_at,
                p.level, p.target_level, p.goal, p.daily_minutes, p.weekly_days, p.timezone
           FROM users u
           JOIN user_profiles p ON p.user_id = u.id
          ORDER BY u.joined_at
          LIMIT 1",
    )?;

    let profile = stmt.query_row([], |row| {
        Ok(UserProfile {
            id: row.get(0)?,
            display_name: row.get(1)?,
            handle: row.get(2)?,
            joined_at: row.get(3)?,
            level: row.get(4)?,
            target_level: row.get(5)?,
            goal: row.get(6)?,
            daily_minutes: row.get(7)?,
            weekly_days: row.get(8)?,
            timezone: row.get(9)?,
        })
    })?;

    Ok(profile)
}

/// 局部更新：只改传入的字段，其余保持不变
pub fn profile_update(conn: &Connection, patch: &Json) -> AppResult<UserProfile> {
    let current = profile_get(conn)?;
    let mut merged = serde_json::to_value(&current)
        .map_err(|e| AppError::internal(format!("资料序列化失败：{e}")))?;
    merge_json(&mut merged, patch.clone());

    let next: UserProfile = serde_json::from_value(merged)
        .map_err(|e| AppError::invalid(format!("资料字段不合法：{e}")))?;

    write_profile(conn, &next, &now_iso())?;
    Ok(next)
}

fn write_profile(conn: &Connection, profile: &UserProfile, now: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE users SET handle = ?2, display_name = ?3, joined_at = ?4, updated_at = ?5
          WHERE id = ?1",
        rusqlite::params![
            profile.id,
            profile.handle,
            profile.display_name,
            profile.joined_at,
            now
        ],
    )?;
    conn.execute(
        "INSERT INTO user_profiles(user_id, level, target_level, goal, daily_minutes, weekly_days, timezone, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(user_id) DO UPDATE SET
           level = excluded.level,
           target_level = excluded.target_level,
           goal = excluded.goal,
           daily_minutes = excluded.daily_minutes,
           weekly_days = excluded.weekly_days,
           timezone = excluded.timezone,
           updated_at = excluded.updated_at",
        rusqlite::params![
            profile.id,
            profile.level,
            profile.target_level,
            profile.goal,
            profile.daily_minutes,
            profile.weekly_days,
            profile.timezone,
            now
        ],
    )?;
    Ok(())
}

/* --------------------------------- 设置 ---------------------------------- */

pub fn settings_get(conn: &Connection) -> AppResult<AppSettings> {
    let mut stmt = conn.prepare("SELECT key, value_json FROM app_settings")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut stored = Map::new();
    for row in rows {
        let (key, raw) = row?;
        let value: Json = serde_json::from_str(&raw).unwrap_or(Json::Null);
        set_nested(&mut stored, &key, value);
    }

    // 默认值兜底：老版本缺字段时不会整体失败
    let mut merged = serde_json::to_value(AppSettings::default())
        .map_err(|e| AppError::internal(format!("设置序列化失败：{e}")))?;
    merge_json(&mut merged, Json::Object(stored));

    serde_json::from_value(merged).map_err(|e| AppError::invalid(format!("设置数据不合法：{e}")))
}

pub fn settings_update(conn: &Connection, patch: &Json) -> AppResult<AppSettings> {
    let current = settings_get(conn)?;
    let mut merged = serde_json::to_value(&current)
        .map_err(|e| AppError::internal(format!("设置序列化失败：{e}")))?;
    merge_json(&mut merged, patch.clone());

    let next: AppSettings = serde_json::from_value(merged)
        .map_err(|e| AppError::invalid(format!("设置字段不合法：{e}")))?;

    write_settings(conn, &next)?;
    Ok(next)
}

fn write_settings(conn: &Connection, settings: &AppSettings) -> AppResult<()> {
    let value = serde_json::to_value(settings)
        .map_err(|e| AppError::internal(format!("设置序列化失败：{e}")))?;

    let mut flat = Map::new();
    flatten_into("", &value, &mut flat);

    let now = now_iso();
    for (key, value) in flat {
        conn.execute(
            "INSERT INTO app_settings(key, value_json, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
                                           updated_at = excluded.updated_at",
            rusqlite::params![key, value.to_string(), now],
        )?;
    }
    Ok(())
}

/* ------------------------------- 本地数据概览 ------------------------------ */

pub fn local_data_summary(conn: &Connection, size_kb: u64) -> AppResult<LocalDataSummary> {
    let count = |sql: &str| -> AppResult<i64> { Ok(conn.query_row(sql, [], |row| row.get(0))?) };

    Ok(LocalDataSummary {
        words: count("SELECT COUNT(*) FROM entity_states WHERE kind IN ('word','phrase')")?,
        grammar_points: count("SELECT COUNT(*) FROM entity_states WHERE kind = 'grammar'")?,
        sentences: count("SELECT COUNT(*) FROM entity_states WHERE kind = 'sentence'")?,
        records: count(
            "SELECT (SELECT COUNT(*) FROM training_sessions)
                  + (SELECT COUNT(*) FROM assessment_attempts)
                  + (SELECT COUNT(*) FROM review_logs)",
        )?,
        size_kb,
    })
}

/// 云同步尚未实现：如实返回"仅本机"，不伪造 `synced`
pub fn sync_status(conn: &Connection, size_kb: u64) -> AppResult<SyncStatus> {
    let last_backup = crate::database::metadata_get(conn, "last_backup_at")?;
    Ok(SyncStatus {
        state: if last_backup.is_some() { "idle" } else { "offline" }.to_string(),
        last_synced_at: None,
        pending_changes: 0,
        device_name: "本机".to_string(),
        storage_used_kb: size_kb,
    })
}

/* --------------------------------- JSON 工具 -------------------------------- */

/// 深合并：对象逐键递归，其余类型由 `patch` 覆盖
fn merge_json(base: &mut Json, patch: Json) {
    match (base, patch) {
        (Json::Object(target), Json::Object(source)) => {
            for (key, value) in source {
                match target.get_mut(&key) {
                    Some(slot) => merge_json(slot, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (slot, value) => *slot = value,
    }
}

fn flatten_into(prefix: &str, value: &Json, out: &mut Map<String, Json>) {
    match value {
        Json::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_into(&path, child, out);
            }
        }
        other => {
            out.insert(prefix.to_string(), other.clone());
        }
    }
}

fn set_nested(map: &mut Map<String, Json>, key: &str, value: Json) {
    let mut parts = key.split('.').peekable();
    let mut cursor = map;

    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            cursor.insert(part.to_string(), value);
            return;
        }
        let entry = cursor
            .entry(part.to_string())
            .or_insert_with(|| Json::Object(Map::new()));
        if !entry.is_object() {
            *entry = Json::Object(Map::new());
        }
        cursor = entry.as_object_mut().expect("刚刚保证是对象");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn seeding_is_idempotent() {
        let db = Database::open_in_memory().expect("open");
        db.with_conn(|conn| {
            ensure_seeded(conn)?;
            ensure_seeded(conn)?;
            let users: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
            assert_eq!(users, 1, "重复初始化不应插入多行");
            Ok(())
        })
        .expect("seed");
    }

    #[test]
    fn profile_patch_only_touches_given_fields() {
        let db = Database::open_in_memory().expect("open");
        let updated = db
            .with_tx(|tx| {
                ensure_seeded(tx)?;
                let patch = serde_json::json!({ "dailyMinutes": 60 });
                profile_update(tx, &patch)
            })
            .expect("update");

        assert_eq!(updated.daily_minutes, 60);
        assert_eq!(updated.level, "B1", "未传入的字段必须保持不变");
        assert_eq!(updated.weekly_days, 6);
    }

    #[test]
    fn settings_roundtrip_and_partial_update() {
        let db = Database::open_in_memory().expect("open");
        let result = db
            .with_tx(|tx| {
                ensure_seeded(tx)?;
                let patch = serde_json::json!({ "appearance": { "theme": "dark" } });
                let next = settings_update(tx, &patch)?;
                let loaded = settings_get(tx)?;
                Ok((next, loaded))
            })
            .expect("settings");

        let (next, loaded) = result;
        assert_eq!(next.appearance.theme, "dark");
        assert_eq!(next.appearance.accent, "brand", "同层其它字段保持默认");
        assert!(next.sound_effects);
        assert_eq!(loaded.appearance.theme, "dark", "落库后可读回");
    }

    #[test]
    fn invalid_settings_patch_is_rejected() {
        let db = Database::open_in_memory().expect("open");
        let outcome = db.with_tx(|tx| {
            ensure_seeded(tx)?;
            let patch = serde_json::json!({ "appearance": { "reduceMotion": "yes" } });
            settings_update(tx, &patch)
        });
        assert!(outcome.is_err(), "类型不符应被拒绝，而不是静默写坏数据");
    }

    #[test]
    fn local_summary_counts_entity_states() {
        let db = Database::open_in_memory().expect("open");
        let summary = db
            .with_conn(|conn| {
                ensure_seeded(conn)?;
                local_data_summary(conn, 42)
            })
            .expect("summary");
        assert_eq!(summary.size_kb, 42);
        assert_eq!(summary.records, 0);
    }
}
