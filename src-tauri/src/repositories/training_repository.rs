//! 训练会话快照与训练事件。
//!
//! 快照是"判分的唯一依据"：开始训练时把题目落库，提交时只按 `sessionId` 读回，
//! 前端无法通过改请求体改变题目或答案。

use std::collections::HashMap;

use rusqlite::Connection;
use serde_json::Value as Json;

use crate::error::AppResult;
use crate::models::exercise::{Exercise, ExerciseAnswer, ExerciseResult, KindScore, TrainingSession};

#[derive(Debug, Clone)]
pub struct SnapshotRow {
    pub session: TrainingSession,
    pub difficulty: i64,
    pub consumed_at: Option<String>,
}

pub fn insert_snapshot(conn: &Connection, session: &TrainingSession, difficulty: i64) -> AppResult<()> {
    let exercises_json = serde_json::to_string(&session.exercises)
        .map_err(|e| crate::error::AppError::internal(format!("题目快照序列化失败：{e}")))?;
    let entities_json = serde_json::to_string(&session.target_entity_ids)
        .map_err(|e| crate::error::AppError::internal(format!("实体快照序列化失败：{e}")))?;
    let now = crate::database::now_iso();

    conn.execute(
        "INSERT INTO training_session_snapshots
           (session_id, kind, track, title, difficulty, estimated_minutes,
            exercises_json, target_entity_ids_json, started_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            session.id,
            session.kind,
            session.track,
            session.title,
            difficulty,
            session.estimated_minutes,
            exercises_json,
            entities_json,
            session.started_at,
            now,
        ],
    )?;
    Ok(())
}

pub fn find_snapshot(conn: &Connection, session_id: &str) -> AppResult<Option<SnapshotRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, kind, track, title, difficulty, estimated_minutes,
                exercises_json, target_entity_ids_json, started_at, consumed_at
           FROM training_session_snapshots WHERE session_id = ?1",
    )?;

    let mut rows = stmt.query([session_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };

    let exercises_raw: String = row.get("exercises_json")?;
    let entities_raw: String = row.get("target_entity_ids_json")?;
    let exercises: Vec<Exercise> = serde_json::from_str(&exercises_raw)
        .map_err(|e| crate::error::AppError::internal(format!("题目快照损坏：{e}")))?;
    let target_entity_ids: Vec<String> = serde_json::from_str(&entities_raw).unwrap_or_default();

    Ok(Some(SnapshotRow {
        session: TrainingSession {
            id: row.get("session_id")?,
            track: row.get("track")?,
            kind: row.get("kind")?,
            title: row.get("title")?,
            exercises,
            started_at: row.get("started_at")?,
            estimated_minutes: row.get("estimated_minutes")?,
            target_entity_ids,
        },
        difficulty: row.get("difficulty")?,
        consumed_at: row.get("consumed_at")?,
    }))
}

pub fn mark_consumed(conn: &Connection, session_id: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE training_session_snapshots SET consumed_at = ?2
          WHERE session_id = ?1 AND consumed_at IS NULL",
        rusqlite::params![session_id, crate::database::now_iso()],
    )?;
    Ok(())
}

/// 每个题目最近一次作答时间（毫秒），用于"最近做过的题降权"
pub fn seen_at(conn: &Connection) -> AppResult<HashMap<String, i64>> {
    let mut stmt = conn.prepare(
        "SELECT a.exercise_id, MAX(s.finished_at)
           FROM training_answers a
           JOIN training_sessions s ON s.id = a.session_id
          GROUP BY a.exercise_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut result = HashMap::new();
    for row in rows {
        let (exercise_id, finished_at) = row?;
        if let Ok(time) = chrono::DateTime::parse_from_rfc3339(&finished_at) {
            result.insert(exercise_id, time.timestamp_millis());
        }
    }
    Ok(result)
}

pub struct AttemptInsert<'a> {
    pub id: &'a str,
    pub submission_id: &'a str,
    pub session_id: &'a str,
    pub kind: &'a str,
    pub track: &'a str,
    pub title: &'a str,
    pub difficulty: i64,
    pub minutes: i64,
    pub duration_ms: i64,
    pub score: f64,
    pub max_score: i64,
    pub accuracy: i64,
    pub xp: i64,
    pub started_at: &'a str,
    pub finished_at: &'a str,
}

pub fn insert_session(conn: &Connection, attempt: &AttemptInsert<'_>) -> AppResult<()> {
    conn.execute(
        "INSERT INTO training_sessions
           (id, submission_id, session_id, kind, track, title, difficulty, minutes,
            duration_ms, score, max_score, accuracy, xp, started_at, finished_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        rusqlite::params![
            attempt.id,
            attempt.submission_id,
            attempt.session_id,
            attempt.kind,
            attempt.track,
            attempt.title,
            attempt.difficulty,
            attempt.minutes,
            attempt.duration_ms,
            attempt.score,
            attempt.max_score,
            attempt.accuracy,
            attempt.xp,
            attempt.started_at,
            attempt.finished_at,
            crate::database::now_iso(),
        ],
    )?;
    Ok(())
}

pub fn insert_answer(conn: &Connection, session_id: &str, answer: &ExerciseAnswer) -> AppResult<()> {
    conn.execute(
        "INSERT INTO training_answers
           (session_id, exercise_id, value_json, correct, partial_score, elapsed_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            session_id,
            answer.exercise_id,
            answer.value.to_string(),
            i64::from(answer.correct),
            answer.partial_score,
            answer.elapsed_ms,
        ],
    )?;
    Ok(())
}

pub fn insert_kind_score(conn: &Connection, session_id: &str, score: &KindScore) -> AppResult<()> {
    conn.execute(
        "INSERT INTO training_kind_scores(session_id, kind, correct, total) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![session_id, score.kind, score.correct, score.total],
    )?;
    Ok(())
}

pub fn insert_session_entity(conn: &Connection, session_id: &str, entity_id: &str) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO training_session_entities(session_id, entity_id) VALUES (?1, ?2)",
        rusqlite::params![session_id, entity_id],
    )?;
    Ok(())
}

/// 幂等命中时重建已有结果（不重新判分、不重复计分）
pub fn find_result_by_submission(
    conn: &Connection,
    submission_id: &str,
) -> AppResult<Option<ExerciseResult>> {
    let head = conn
        .query_row(
            "SELECT id, session_id, track, score, max_score, accuracy, duration_ms, xp, finished_at
               FROM training_sessions WHERE submission_id = ?1",
            [submission_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .ok();

    let Some((id, session_id, track, score, max_score, accuracy, duration_ms, xp, finished_at)) = head
    else {
        return Ok(None);
    };

    let mut answer_stmt = conn.prepare(
        "SELECT exercise_id, value_json, correct, partial_score, elapsed_ms
           FROM training_answers WHERE session_id = ?1 ORDER BY id",
    )?;
    let answers = answer_stmt
        .query_map([id.as_str()], |row| {
            let raw: String = row.get(1)?;
            Ok(ExerciseAnswer {
                exercise_id: row.get(0)?,
                value: serde_json::from_str(&raw).unwrap_or(Json::Null),
                correct: row.get::<_, i64>(2)? != 0,
                partial_score: row.get(3)?,
                elapsed_ms: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut kind_stmt = conn.prepare(
        "SELECT kind, correct, total FROM training_kind_scores WHERE session_id = ?1 ORDER BY kind",
    )?;
    let per_kind = kind_stmt
        .query_map([id.as_str()], |row| {
            Ok(KindScore {
                kind: row.get(0)?,
                correct: row.get(1)?,
                total: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut weak_stmt = conn.prepare(
        "SELECT DISTINCT ee.entity_id
           FROM training_answers a
           JOIN exercise_entities ee ON ee.exercise_id = a.exercise_id
          WHERE a.session_id = ?1 AND a.partial_score < 0.7
          ORDER BY ee.entity_id",
    )?;
    let weak_entity_ids = weak_stmt
        .query_map([id.as_str()], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(Some(ExerciseResult {
        session_id,
        track,
        answers,
        score,
        max_score,
        accuracy,
        duration_ms,
        xp,
        per_kind,
        weak_entity_ids,
        finished_at,
        // 能走到这里说明是回读既有结果，即重复提交
        duplicate: true,
    }))
}

/// 累计训练经验（首页 XP 用）
pub fn total_xp(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COALESCE(SUM(xp), 0) FROM training_sessions", [], |row| {
        row.get(0)
    })?)
}
