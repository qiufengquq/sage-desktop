//! 内容包同步。
//!
//! 内容（词库 / 语法 / 句子 / 关系 / 题库 / 测评结构 / 训练入口）的唯一事实源是
//! 前端 TS 模块（`src/services/mock/*`），由 `scripts/export-content.mjs` 导出成
//! `content_pack.json` 后编译进二进制。Rust 不手写内容，也不允许手改这个 JSON。
//!
//! 同步是幂等的：按主键/唯一键 `INSERT ... ON CONFLICT DO UPDATE`，
//! 版本号未变化时直接跳过。内容只增不删（旧行保留，避免历史事件失去引用）。

use rusqlite::types::Value as SqlValue;
use rusqlite::{params_from_iter, Connection, TransactionBehavior};
use serde::Deserialize;
use serde_json::Value as Json;

use crate::database::{metadata_get, metadata_set, now_iso};
use crate::error::{AppError, AppResult};

const PACK_JSON: &str = include_str!("../content/content_pack.json");
const VERSION_KEY: &str = "content_pack_version";
const CHECKSUM_KEY: &str = "content_pack_checksum";

/// 同步目标：表名 + 冲突键 + 是否需要自动填充时间列
struct Target {
    table: &'static str,
    conflict: &'static [&'static str],
    timestamps: bool,
}

/// 顺序即外键依赖顺序：父表在前
const TARGETS: &[Target] = &[
    Target { table: "entities", conflict: &["id"], timestamps: true },
    Target { table: "vocabulary", conflict: &["entity_id"], timestamps: false },
    Target { table: "word_meanings", conflict: &["entity_id", "sort_order"], timestamps: false },
    Target { table: "word_examples", conflict: &["id"], timestamps: false },
    Target { table: "grammar_points", conflict: &["entity_id"], timestamps: false },
    Target { table: "grammar_examples", conflict: &["id"], timestamps: false },
    Target { table: "grammar_mistakes", conflict: &["id"], timestamps: false },
    Target { table: "sentences", conflict: &["entity_id"], timestamps: false },
    Target { table: "knowledge_relations", conflict: &["source_id", "target_id", "relation"], timestamps: false },
    Target { table: "training_options", conflict: &["id"], timestamps: false },
    Target { table: "exercises", conflict: &["id"], timestamps: false },
    Target { table: "exercise_entities", conflict: &["exercise_id", "entity_id"], timestamps: false },
    Target { table: "assessment_modules", conflict: &["id"], timestamps: false },
    Target { table: "assessment_sections", conflict: &["id"], timestamps: false },
    Target { table: "assessment_section_exercises", conflict: &["section_id", "exercise_id"], timestamps: false },
];

#[derive(Debug, Deserialize)]
struct ContentPack {
    version: String,
    #[serde(default)]
    tables: Json,
}

#[derive(Debug, Clone)]
pub struct SyncReport {
    pub version: String,
    pub skipped: bool,
    pub rows: usize,
}

fn json_to_sql(value: &Json) -> SqlValue {
    match value {
        Json::Null => SqlValue::Null,
        Json::Bool(flag) => SqlValue::Integer(i64::from(*flag)),
        Json::Number(number) => number
            .as_i64()
            .map(SqlValue::Integer)
            .or_else(|| number.as_f64().map(SqlValue::Real))
            .unwrap_or(SqlValue::Null),
        Json::String(text) => SqlValue::Text(text.clone()),
        // 数组 / 对象统一存成 JSON 文本（*_json 列）
        other => SqlValue::Text(other.to_string()),
    }
}

fn upsert_rows(tx: &Connection, target: &Target, rows: &[Json]) -> AppResult<usize> {
    if rows.is_empty() {
        return Ok(0);
    }

    let mut written = 0usize;
    let now = now_iso();

    for row in rows {
        let object = row
            .as_object()
            .ok_or_else(|| AppError::internal(format!("内容包 {} 的行不是对象", target.table)))?;

        let mut columns: Vec<String> = object.keys().cloned().collect();
        let mut values: Vec<SqlValue> = object.values().map(json_to_sql).collect();

        if target.timestamps {
            for column in ["created_at", "updated_at"] {
                if !columns.iter().any(|c| c == column) {
                    columns.push(column.to_string());
                    values.push(SqlValue::Text(now.clone()));
                }
            }
        }

        let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("?{i}")).collect();
        let updatable: Vec<&String> = columns
            .iter()
            .filter(|column| {
                !target.conflict.contains(&column.as_str()) && column.as_str() != "created_at"
            })
            .collect();

        let conflict_cols = target.conflict.join(", ");
        let sql = if updatable.is_empty() {
            format!(
                "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({conflict_cols}) DO NOTHING",
                target.table,
                columns.join(", "),
                placeholders.join(", ")
            )
        } else {
            let assignments: Vec<String> = updatable
                .iter()
                .map(|column| format!("{column} = excluded.{column}"))
                .collect();
            format!(
                "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({conflict_cols}) DO UPDATE SET {}",
                target.table,
                columns.join(", "),
                placeholders.join(", "),
                assignments.join(", ")
            )
        };

        tx.execute(&sql, params_from_iter(values.iter()))?;
        written += 1;
    }

    Ok(written)
}

/// 同步编译期内嵌的内容包（版本未变则跳过）
pub fn sync(conn: &mut Connection) -> AppResult<SyncReport> {
    sync_inner(conn, false)
}

/// 强制重新写入（内容包版本号未变但内容被修正时使用）
pub fn sync_force(conn: &mut Connection) -> AppResult<SyncReport> {
    sync_inner(conn, true)
}

fn sync_inner(conn: &mut Connection, force: bool) -> AppResult<SyncReport> {
    let pack: ContentPack = serde_json::from_str(PACK_JSON)
        .map_err(|e| AppError::internal(format!("内容包不是合法 JSON：{e}")))?;

    if !force {
        if let Some(current) = metadata_get(conn, VERSION_KEY)? {
            if current == pack.version {
                return Ok(SyncReport { version: pack.version, skipped: true, rows: 0 });
            }
        }
    }

    let tables = pack
        .tables
        .as_object()
        .ok_or_else(|| AppError::internal("内容包缺少 tables 字段"))?;

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut total = 0usize;
    for target in TARGETS {
        let Some(rows) = tables.get(target.table) else {
            continue;
        };
        let rows = rows
            .as_array()
            .ok_or_else(|| AppError::internal(format!("内容包 {} 不是数组", target.table)))?;
        total += upsert_rows(&tx, target, rows)?;
    }

    let checksum = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(PACK_JSON.as_bytes());
        format!("{:x}", hasher.finalize())
    };
    metadata_set(&tx, VERSION_KEY, &pack.version)?;
    metadata_set(&tx, CHECKSUM_KEY, &checksum)?;
    if metadata_get(&tx, "seeded_at")?.is_none() {
        metadata_set(&tx, "seeded_at", &now_iso())?;
    }
    tx.commit()?;

    Ok(SyncReport { version: pack.version, skipped: false, rows: total })
}

/// 当前内容包的版本号（前端"本地数据"页展示用）
pub fn pack_version() -> AppResult<String> {
    let pack: ContentPack = serde_json::from_str(PACK_JSON)
        .map_err(|e| AppError::internal(format!("内容包不是合法 JSON：{e}")))?;
    Ok(pack.version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{migrate, pragmas};

    fn fresh_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("mem");
        pragmas::apply(&conn).expect("pragmas");
        migrate::run(&mut conn, std::path::Path::new(":memory:")).expect("migrate");
        conn
    }

    fn table_count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
            .expect("count")
    }

    #[test]
    fn content_pack_is_self_consistent() {
        let pack: ContentPack = serde_json::from_str(PACK_JSON).expect("合法 JSON");
        assert!(!pack.version.is_empty(), "内容包缺少版本号");
        let tables = pack.tables.as_object().expect("tables");
        assert!(tables.contains_key("entities"), "内容包缺少 entities");
        assert!(tables.contains_key("exercises"), "内容包缺少 exercises");
    }

    #[test]
    fn sync_writes_rows_and_is_idempotent() {
        let mut conn = fresh_conn();

        let first = sync(&mut conn).expect("first sync");
        assert!(!first.skipped, "首次同步不应跳过");

        let entities = table_count(&conn, "entities");
        let exercises = table_count(&conn, "exercises");
        assert!(entities > 0, "entities 为空");
        assert!(exercises > 0, "exercises 为空");

        // 版本未变 → 跳过
        let second = sync(&mut conn).expect("second sync");
        assert!(second.skipped, "版本未变应跳过");

        // 强制同步 → 行数不变，说明是 UPSERT 而不是重复插入
        sync_force(&mut conn).expect("force sync");
        assert_eq!(table_count(&conn, "entities"), entities);
        assert_eq!(table_count(&conn, "exercises"), exercises);
    }

    #[test]
    fn relations_reference_existing_entities() {
        let mut conn = fresh_conn();
        sync(&mut conn).expect("sync");

        let orphans: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM knowledge_relations r
                  WHERE NOT EXISTS (SELECT 1 FROM entities e WHERE e.id = r.source_id)
                     OR NOT EXISTS (SELECT 1 FROM entities e WHERE e.id = r.target_id)",
                [],
                |row| row.get(0),
            )
            .expect("orphans");
        assert_eq!(orphans, 0, "内容包里的关系引用了不存在的实体");
    }
}
