//! 测评内容与测评事件。

use rusqlite::Connection;

use crate::error::AppResult;
use crate::models::assessment::{
    AssessmentModule, AssessmentOutcome, AssessmentSection, FollowUp, LastAttempt, SectionOutcome,
};
use crate::repositories::exercise_repository;

/// 三个模块 + 分项 + 题目，并附上最近一次成绩
pub fn modules(conn: &Connection) -> AppResult<Vec<AssessmentModule>> {
    let mut module_stmt = conn.prepare(
        "SELECT id, title, subtitle, description, minutes
           FROM assessment_modules ORDER BY sort_order",
    )?;
    let module_rows = module_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut result = Vec::with_capacity(module_rows.len());
    for (id, title, subtitle, description, minutes) in module_rows {
        let sections = sections_of(conn, &id)?;
        let last_attempt = latest_attempt(conn, &id)?;
        result.push(AssessmentModule {
            id,
            title,
            subtitle,
            description,
            minutes,
            sections,
            last_attempt,
        });
    }
    Ok(result)
}

pub fn find_module(conn: &Connection, module_id: &str) -> AppResult<Option<AssessmentModule>> {
    Ok(modules(conn)?
        .into_iter()
        .find(|module| module.id == module_id))
}

fn sections_of(conn: &Connection, module_id: &str) -> AppResult<Vec<AssessmentSection>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, description, kind, skill
           FROM assessment_sections WHERE module_id = ?1 ORDER BY sort_order",
    )?;
    let rows = stmt
        .query_map([module_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut sections = Vec::with_capacity(rows.len());
    for (id, title, description, kind, skill) in rows {
        let mut item_stmt = conn.prepare(
            "SELECT exercise_id FROM assessment_section_exercises
              WHERE section_id = ?1 ORDER BY sort_order",
        )?;
        let item_ids = item_stmt
            .query_map([id.as_str()], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut items = Vec::with_capacity(item_ids.len());
        for exercise_id in item_ids {
            if let Some(exercise) = exercise_repository::find(conn, &exercise_id)? {
                items.push(exercise);
            }
        }

        sections.push(AssessmentSection {
            id,
            title,
            description,
            kind,
            skill,
            items,
        });
    }
    Ok(sections)
}

fn latest_attempt(conn: &Connection, module_id: &str) -> AppResult<Option<LastAttempt>> {
    let mut stmt = conn.prepare(
        "SELECT finished_at, score FROM assessment_attempts
          WHERE module_id = ?1 ORDER BY finished_at DESC, id DESC LIMIT 1",
    )?;
    let mut rows = stmt.query([module_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(LastAttempt {
            at: row.get(0)?,
            score: row.get(1)?,
        })),
        None => Ok(None),
    }
}

pub struct AttemptInsert<'a> {
    pub id: &'a str,
    pub submission_id: &'a str,
    pub module_id: &'a str,
    pub score: i64,
    pub accuracy: i64,
    pub level: &'a str,
    pub strengths: &'a [String],
    pub weaknesses: &'a [String],
    pub recommendation: &'a str,
    pub minutes: i64,
    pub finished_at: &'a str,
}

pub fn insert_attempt(conn: &Connection, attempt: &AttemptInsert<'_>) -> AppResult<()> {
    let strengths = serde_json::to_string(attempt.strengths).unwrap_or_else(|_| "[]".into());
    let weaknesses = serde_json::to_string(attempt.weaknesses).unwrap_or_else(|_| "[]".into());

    conn.execute(
        "INSERT INTO assessment_attempts
           (id, submission_id, module_id, score, accuracy, level, strengths_json, weaknesses_json,
            recommendation, minutes, finished_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            attempt.id,
            attempt.submission_id,
            attempt.module_id,
            attempt.score,
            attempt.accuracy,
            attempt.level,
            strengths,
            weaknesses,
            attempt.recommendation,
            attempt.minutes,
            attempt.finished_at,
            crate::database::now_iso(),
        ],
    )?;
    Ok(())
}

pub fn insert_section_result(
    conn: &Connection,
    attempt_id: &str,
    section: &SectionOutcome,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO assessment_section_results
           (attempt_id, section_id, title, correct, total, score)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            attempt_id,
            section.section_id,
            section.title,
            section.correct,
            section.total,
            section.score,
        ],
    )?;
    Ok(())
}

pub fn insert_answer(
    conn: &Connection,
    attempt_id: &str,
    answer: &crate::models::exercise::ExerciseAnswer,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO assessment_answers
           (attempt_id, exercise_id, value_json, correct, partial_score, elapsed_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            attempt_id,
            answer.exercise_id,
            answer.value.to_string(),
            i64::from(answer.correct),
            answer.partial_score,
            answer.elapsed_ms,
        ],
    )?;
    Ok(())
}

/// 幂等命中时重建结果（不重新判分）
pub fn find_outcome_by_submission(
    conn: &Connection,
    submission_id: &str,
) -> AppResult<Option<AssessmentOutcome>> {
    let head = conn
        .query_row(
            "SELECT id, module_id, score, accuracy, level, strengths_json, weaknesses_json,
                    recommendation, finished_at
               FROM assessment_attempts WHERE submission_id = ?1",
            [submission_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .ok();

    let Some((id, module_id, score, accuracy, level, strengths, weaknesses, recommendation, finished_at)) =
        head
    else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT section_id, title, correct, total, score
           FROM assessment_section_results WHERE attempt_id = ?1 ORDER BY section_id",
    )?;
    let per_section = stmt
        .query_map([id.as_str()], |row| {
            Ok(SectionOutcome {
                section_id: row.get(0)?,
                title: row.get(1)?,
                correct: row.get(2)?,
                total: row.get(3)?,
                score: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // followUp 不落库：由最弱分项 + 模块训练方向重新推算
    let module = find_module(conn, &module_id)?;
    let follow_up = weakest_follow_up(&module_id, &per_section, module.as_ref());

    Ok(Some(AssessmentOutcome {
        module_id,
        score,
        accuracy,
        level,
        per_section,
        strengths: serde_json::from_str(&strengths).unwrap_or_default(),
        weaknesses: serde_json::from_str(&weaknesses).unwrap_or_default(),
        recommendation,
        finished_at,
        follow_up,
        // 能走到这里说明是回读既有结果，即重复提交
        duplicate: true,
    }))
}

/// 最弱分项 → "去训练"入口
pub fn weakest_follow_up(
    module_id: &str,
    per_section: &[SectionOutcome],
    module: Option<&AssessmentModule>,
) -> Option<FollowUp> {
    let scored: Vec<&SectionOutcome> = per_section.iter().filter(|item| item.total > 0).collect();
    let weakest = scored.iter().min_by_key(|item| item.score)?;
    let section = module?.sections.iter().find(|item| item.id == weakest.section_id)?;

    Some(FollowUp {
        label: format!("训练{}", weakest.title),
        kind: section.kind.clone(),
        track: module_track(module_id).to_string(),
    })
}

pub fn module_track(module_id: &str) -> &'static str {
    match module_id {
        "grammar" => "grammar",
        "speaking" => "speaking",
        _ => "vocabulary",
    }
}

/// 供能力画像使用：所有测评的精简列表
pub fn assessment_summaries(
    conn: &Connection,
) -> AppResult<Vec<crate::algorithms::ability::AssessmentSummary>> {
    let mut stmt = conn.prepare(
        "SELECT id, module_id, score, finished_at FROM assessment_attempts ORDER BY finished_at",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(crate::algorithms::ability::AssessmentSummary {
            id: row.get(0)?,
            module_id: row.get(1)?,
            score: row.get(2)?,
            finished_at: row.get(3)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}
