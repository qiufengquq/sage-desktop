//! 题库读取：训练入口、取题池、按 id 取题。

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{Map, Value as Json};

use crate::error::AppResult;
use crate::models::exercise::{Exercise, ExercisePrompt, TrainingOption};

const EXERCISE_COLUMNS: &str =
    "id, kind, difficulty, xp, instruction, context, explanation, tags_json, payload_json";

fn row_to_exercise(row: &Row<'_>) -> rusqlite::Result<Exercise> {
    let tags_raw: String = row.get("tags_json")?;
    let payload_raw: String = row.get("payload_json")?;

    let payload = serde_json::from_str::<Json>(&payload_raw)
        .ok()
        .filter(Json::is_object)
        .unwrap_or_else(|| Json::Object(Map::new()));

    Ok(Exercise {
        id: row.get("id")?,
        kind: row.get("kind")?,
        difficulty: row.get("difficulty")?,
        xp: row.get("xp")?,
        prompt: ExercisePrompt {
            instruction: row.get("instruction")?,
            context: row.get("context")?,
        },
        entity_ids: Vec::new(),
        explanation: row.get("explanation")?,
        tags: serde_json::from_str(&tags_raw).unwrap_or_default(),
        payload,
    })
}

/// 把 `exercise_entities` 填进题目（前端用 `entityIds` 做掌握度回写与跳转）
fn attach_entities(conn: &Connection, exercise: &mut Exercise) -> AppResult<()> {
    let mut stmt =
        conn.prepare("SELECT entity_id FROM exercise_entities WHERE exercise_id = ?1 ORDER BY entity_id")?;
    let rows = stmt.query_map([exercise.id.as_str()], |row| row.get::<_, String>(0))?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(row?);
    }
    exercise.entity_ids = ids;
    Ok(())
}

pub fn pool(conn: &Connection, kind: &str) -> AppResult<Vec<Exercise>> {
    let sql = format!(
        "SELECT {EXERCISE_COLUMNS} FROM exercises
          WHERE kind = ?1 AND deprecated_at IS NULL
          ORDER BY rowid"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([kind], row_to_exercise)?;

    let mut result = Vec::new();
    for row in rows {
        let mut exercise = row?;
        attach_entities(conn, &mut exercise)?;
        result.push(exercise);
    }
    Ok(result)
}

pub fn find(conn: &Connection, id: &str) -> AppResult<Option<Exercise>> {
    let sql = format!("SELECT {EXERCISE_COLUMNS} FROM exercises WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(row) => {
            let mut exercise = row_to_exercise(row)?;
            attach_entities(conn, &mut exercise)?;
            Ok(Some(exercise))
        }
        None => Ok(None),
    }
}

/// 训练入口列表。`itemCount` 取"声明题量"与"题库实际容量"的较小值，
/// 避免出现"入口说有 5 题、实际只有 2 题"。
pub fn options(conn: &Connection, track: Option<&str>) -> AppResult<Vec<TrainingOption>> {
    let mut stmt = conn.prepare(
        "SELECT o.id, o.track, o.kind, o.title, o.description, o.icon, o.minutes,
                o.item_count, o.difficulty,
                (SELECT COUNT(*) FROM exercises e
                   WHERE e.kind = o.kind AND e.deprecated_at IS NULL) AS capacity,
                (SELECT COUNT(DISTINCT a.exercise_id)
                   FROM training_answers a
                   JOIN exercises e2 ON e2.id = a.exercise_id
                  WHERE e2.kind = o.kind) AS answered
           FROM training_options o
          ORDER BY o.sort_order",
    )?;

    let rows = stmt.query_map([], |row| {
        let declared: i64 = row.get("item_count")?;
        let capacity: i64 = row.get("capacity")?;
        let answered: i64 = row.get("answered")?;
        let item_count = declared.min(capacity).max(1);
        let progress = if capacity > 0 {
            answered as f64 / capacity as f64
        } else {
            0.0
        };

        Ok(TrainingOption {
            id: row.get("id")?,
            track: row.get("track")?,
            kind: row.get("kind")?,
            title: row.get("title")?,
            description: row.get("description")?,
            icon: row.get("icon")?,
            minutes: row.get("minutes")?,
            item_count,
            difficulty: row.get("difficulty")?,
            progress,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        let option = row?;
        if let Some(track) = track {
            if option.track != track {
                continue;
            }
        }
        result.push(option);
    }
    Ok(result)
}

/// 某题型入口的声明题量（训练开始时按入口题量取题）。
///
/// `training_options.kind` **不是唯一键**：同一题型可以在不同 track 各有一个入口
/// （例如 `multiple-choice` 同时属于 vocabulary 与 grammar）。因此必须叠加 track
/// 过滤，并用 `sort_order` 兜底，保证同一入参永远得到同一行。
pub fn option_item_count(
    conn: &Connection,
    kind: &str,
    track: Option<&str>,
) -> AppResult<Option<i64>> {
    let value = conn
        .query_row(
            "SELECT item_count FROM training_options
              WHERE kind = ?1 AND (?2 IS NULL OR track = ?2)
              ORDER BY sort_order
              LIMIT 1",
            params![kind, track],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(value)
}
