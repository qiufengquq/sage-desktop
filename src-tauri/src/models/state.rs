//! 知识实体的学习态。对应 `src/types/learning-state.ts::EntityState`。
//!
//! 一个实体一行，既是"当前掌握度"的唯一来源，也是 SM-2 调度器的状态载体。

use rusqlite::Row;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityState {
    pub entity_id: String,
    pub kind: String,
    /// 0–100
    pub mastery: f64,
    /// 0–1
    pub strength: f64,
    pub state: String,
    pub review_count: i64,
    pub lapse_count: i64,
    /// SM-2 难度因子，1.3–2.8
    pub ease: f64,
    pub repetition: i64,
    pub interval_days: i64,
    pub due_at: Option<String>,
    pub last_reviewed_at: Option<String>,
    pub updated_at: String,
}

impl EntityState {
    pub fn new(entity_id: &str, kind: &str, now: &str) -> Self {
        Self {
            entity_id: entity_id.to_string(),
            kind: kind.to_string(),
            mastery: 0.0,
            strength: 0.0,
            state: "new".to_string(),
            review_count: 0,
            lapse_count: 0,
            ease: 2.5,
            repetition: 0,
            interval_days: 0,
            due_at: None,
            last_reviewed_at: None,
            updated_at: now.to_string(),
        }
    }

    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            entity_id: row.get("entity_id")?,
            kind: row.get("kind")?,
            mastery: row.get("mastery")?,
            strength: row.get("strength")?,
            state: row.get("state")?,
            review_count: row.get("review_count")?,
            lapse_count: row.get("lapse_count")?,
            ease: row.get("ease")?,
            repetition: row.get("repetition")?,
            interval_days: row.get("interval_days")?,
            due_at: row.get("due_at")?,
            last_reviewed_at: row.get("last_reviewed_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

/// 复习队列项：学习态 + 内容摘要（由 SQL 联查得到）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DueItem {
    #[serde(flatten)]
    pub state: EntityState,
    pub label: String,
    pub secondary: String,
    pub difficulty: i64,
}
