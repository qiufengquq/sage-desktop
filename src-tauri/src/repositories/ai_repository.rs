//! AI Provider 配置仓储（`ai_providers`）。
//!
//! 只存非密钥字段。API Key 由 `crate::ai::secrets` 管理，本模块不接触。

use rusqlite::Connection;

use crate::error::AppResult;
use crate::models::ai::AiProviderRecord;

const COLUMNS: &str = "id, display_name, protocol, base_url, enabled, default_model, \
                       models_json, has_api_key, updated_at";

pub fn load_all(conn: &Connection) -> AppResult<Vec<AiProviderRecord>> {
    let sql = format!("SELECT {COLUMNS} FROM ai_providers ORDER BY id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        let models_raw: String = row.get(6)?;
        Ok(AiProviderRecord {
            id: row.get(0)?,
            display_name: row.get(1)?,
            protocol: row.get(2)?,
            base_url: row.get(3)?,
            enabled: row.get::<_, i64>(4)? != 0,
            default_model: row.get(5)?,
            models: serde_json::from_str(&models_raw).unwrap_or_default(),
            has_api_key: row.get::<_, i64>(7)? != 0,
            updated_at: row.get(8)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn upsert(conn: &Connection, record: &AiProviderRecord) -> AppResult<()> {
    let models_json = serde_json::to_string(&record.models).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO ai_providers
           (id, display_name, protocol, base_url, enabled, default_model,
            models_json, has_api_key, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET
           display_name  = excluded.display_name,
           protocol      = excluded.protocol,
           base_url      = excluded.base_url,
           enabled       = excluded.enabled,
           default_model = excluded.default_model,
           models_json   = excluded.models_json,
           has_api_key   = excluded.has_api_key,
           updated_at    = excluded.updated_at",
        rusqlite::params![
            record.id,
            record.display_name,
            record.protocol,
            record.base_url,
            i64::from(record.enabled),
            record.default_model,
            models_json,
            i64::from(record.has_api_key),
            record.updated_at,
        ],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<usize> {
    Ok(conn.execute("DELETE FROM ai_providers WHERE id = ?1", [id])?)
}

pub fn count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM ai_providers", [], |row| row.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn record(id: &str) -> AiProviderRecord {
        AiProviderRecord {
            id: id.to_string(),
            display_name: "DeepSeek".to_string(),
            protocol: "openai-compatible".to_string(),
            base_url: "https://api.deepseek.com".to_string(),
            enabled: true,
            default_model: Some("deepseek-chat".to_string()),
            models: vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
            has_api_key: true,
            updated_at: "2026-11-22T00:00:00.000Z".to_string(),
        }
    }

    #[test]
    fn upsert_then_load_roundtrip() {
        let db = Database::open_in_memory().expect("open");
        db.with_tx(|tx| {
            upsert(tx, &record("deepseek"))?;
            let loaded = load_all(tx)?;
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].models.len(), 2);
            assert!(loaded[0].enabled);
            assert!(loaded[0].has_api_key);
            Ok(())
        })
        .expect("roundtrip");
    }

    #[test]
    fn upsert_updates_existing_row() {
        let db = Database::open_in_memory().expect("open");
        db.with_tx(|tx| {
            upsert(tx, &record("deepseek"))?;
            let mut next = record("deepseek");
            next.display_name = "DeepSeek 官方".to_string();
            next.has_api_key = false;
            upsert(tx, &next)?;

            let loaded = load_all(tx)?;
            assert_eq!(loaded.len(), 1, "同 id 不得产生第二行");
            assert_eq!(loaded[0].display_name, "DeepSeek 官方");
            assert!(!loaded[0].has_api_key);
            Ok(())
        })
        .expect("update");
    }

    #[test]
    fn delete_removes_row() {
        let db = Database::open_in_memory().expect("open");
        db.with_tx(|tx| {
            upsert(tx, &record("a"))?;
            assert_eq!(delete(tx, "a")?, 1);
            assert_eq!(count(tx)?, 0);
            Ok(())
        })
        .expect("delete");
    }
}
