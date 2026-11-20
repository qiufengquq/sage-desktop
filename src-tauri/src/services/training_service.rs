//! 训练服务：取题、判分、写回。
//!
//! 提交是本阶段第一条真正的写事务，边界如下（全部在一个事务内）：
//!   幂等检查 → 读会话快照 → 判分 → 写训练事件 → 更新掌握度与调度 → 写掌握度账本
//!   → 标记会话已消费 → 递增 db_revision → COMMIT
//! 任一步失败整体回滚，不会出现"记录了成绩但没更新掌握度"的中间态。

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::Utc;
use rusqlite::Transaction;

use crate::algorithms::{grading, selection};
use crate::database::now_iso;
use crate::error::{AppError, AppResult};
use crate::models::exercise::{
    Exercise, ExerciseAnswer, ExerciseResult, KindScore, TrainingOption, TrainingSession,
};
use crate::models::state::EntityState;
use crate::repositories::{
    exercise_repository, mastery_repository, state_repository, training_repository,
};

const PARTIAL_THRESHOLD: f64 = 0.7;

pub struct StartInput {
    pub kind: String,
    pub track: String,
    pub count: Option<i64>,
    pub title: Option<String>,
    pub difficulty: Option<f64>,
}

pub fn options(
    conn: &rusqlite::Connection,
    track: Option<&str>,
) -> AppResult<Vec<TrainingOption>> {
    exercise_repository::options(conn, track)
}

/* ---------------------------------- 开始 ---------------------------------- */

pub fn start(tx: &Transaction<'_>, input: StartInput) -> AppResult<TrainingSession> {
    let declared =
        exercise_repository::option_item_count(tx, &input.kind, Some(&input.track))?.unwrap_or(4);
    let count = input.count.unwrap_or(declared).max(1) as usize;

    let target_difficulty = match input.difficulty {
        Some(value) => value,
        None => active_plan_difficulty(tx)?.unwrap_or(3.0),
    };

    let pool = exercise_repository::pool(tx, &input.kind)?;
    if pool.is_empty() {
        return Err(AppError::not_found(format!("题型 {} 的题目", input.kind)));
    }

    let now = Utc::now();
    let seen = training_repository::seen_at(tx)?;
    let picked = selection::select_exercises(
        &pool,
        count,
        target_difficulty,
        &seen,
        now.timestamp_millis(),
    );

    let title = input
        .title
        .filter(|value| !value.trim().is_empty())
        .or_else(|| picked.first().map(|exercise| exercise.prompt.instruction.clone()))
        .unwrap_or_else(|| "训练".to_string());

    let mut entity_ids: Vec<String> = Vec::new();
    for exercise in &picked {
        for entity_id in &exercise.entity_ids {
            if !entity_ids.contains(entity_id) {
                entity_ids.push(entity_id.clone());
            }
        }
    }

    let session = TrainingSession {
        id: create_id("sess"),
        track: input.track,
        kind: input.kind,
        title,
        estimated_minutes: ((picked.len() as f64 * 58.0) / 60.0).round().max(2.0) as i64,
        exercises: picked,
        started_at: now_iso(),
        target_entity_ids: entity_ids,
    };

    let difficulty = (target_difficulty.round() as i64).clamp(1, 5);
    // 快照存**完整**题目（含答案），判分与逐题校验都以它为准
    training_repository::insert_snapshot(tx, &session, difficulty)?;

    // 下发给客户端的是去掉答案的形态
    Ok(TrainingSession {
        exercises: session.exercises.iter().map(Exercise::to_player).collect(),
        ..session
    })
}

fn active_plan_difficulty(conn: &rusqlite::Connection) -> AppResult<Option<f64>> {
    let value: Option<f64> = conn
        .query_row(
            "SELECT COALESCE(difficulty_override, difficulty_current)
               FROM study_plans WHERE status = 'active' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    Ok(value)
}

/* ---------------------------------- 提交 ---------------------------------- */

pub fn submit(
    tx: &Transaction<'_>,
    session_id: &str,
    answers: &[ExerciseAnswer],
) -> AppResult<ExerciseResult> {
    // 1. 幂等：同一会话重复提交直接返回既有结果，不重复计分
    if let Some(existing) = training_repository::find_result_by_submission(tx, session_id)? {
        return Ok(existing);
    }

    // 2. 取会话快照（判分依据）
    let snapshot = training_repository::find_snapshot(tx, session_id)?
        .ok_or_else(|| AppError::not_found("训练会话"))?;

    if snapshot.consumed_at.is_some() {
        return Err(AppError::new("invalid_argument", "该训练会话已经提交过"));
    }

    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    let finished_at = now_iso();
    let answer_by_id: HashMap<&str, &ExerciseAnswer> = answers
        .iter()
        .map(|answer| (answer.exercise_id.as_str(), answer))
        .collect();

    // 3. 判分（完全忽略前端给的 correct/partialScore）
    let mut total: i64 = 0;
    let mut earned = 0.0f64;
    let mut duration_ms: i64 = 0;
    let mut xp_earned = 0.0f64;
    let mut per_kind: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut weak_entities: BTreeSet<String> = BTreeSet::new();
    let mut graded: Vec<ExerciseAnswer> = Vec::new();
    // 判分结果同时用于"写答案"和"更新掌握度"，只算一次，避免两处逻辑漂移
    let mut partial_by_exercise: HashMap<String, f64> = HashMap::new();

    for exercise in &snapshot.session.exercises {
        let answer = answer_by_id.get(exercise.id.as_str());
        let outcome = match answer {
            Some(answer) => grading::score_answer(&exercise.kind, exercise.payload(), &answer.value),
            None => grading::ScoreOutcome { correct: false, partial: 0.0 },
        };
        partial_by_exercise.insert(exercise.id.clone(), outcome.partial);

        total += 1;
        earned += outcome.partial;
        xp_earned += exercise.xp as f64 * outcome.partial;
        duration_ms += answer.map(|item| item.elapsed_ms).unwrap_or(0);

        let bucket = per_kind.entry(exercise.kind.clone()).or_insert((0, 0));
        bucket.1 += 1;
        if outcome.correct {
            bucket.0 += 1;
        }

        if outcome.partial < PARTIAL_THRESHOLD {
            for entity_id in &exercise.entity_ids {
                weak_entities.insert(entity_id.clone());
            }
        }

        if let Some(answer) = answer {
            graded.push(ExerciseAnswer {
                exercise_id: exercise.id.clone(),
                value: answer.value.clone(),
                correct: outcome.correct,
                partial_score: outcome.partial,
                elapsed_ms: answer.elapsed_ms,
            });
        }
    }

    let accuracy = if total == 0 {
        0
    } else {
        ((earned / total as f64) * 100.0).round() as i64
    };
    let score = crate::algorithms::util::round1(earned);
    let xp = xp_earned.round() as i64;
    let difficulty = snapshot
        .session
        .exercises
        .first()
        .map(|exercise| exercise.difficulty)
        .unwrap_or(3);
    let minutes = ((duration_ms as f64 / 60_000.0).round() as i64).max(1);

    // 4. 写训练事件
    let attempt_id = format!("ta_{now_ms}");
    training_repository::insert_session(
        tx,
        &training_repository::AttemptInsert {
            id: &attempt_id,
            submission_id: session_id,
            session_id: &snapshot.session.id,
            kind: &snapshot.session.kind,
            track: &snapshot.session.track,
            title: &snapshot.session.title,
            difficulty,
            minutes,
            duration_ms,
            score,
            max_score: total,
            accuracy,
            xp,
            started_at: &snapshot.session.started_at,
            finished_at: &finished_at,
        },
    )?;

    for answer in &graded {
        training_repository::insert_answer(tx, &attempt_id, answer)?;
    }
    for (kind, (correct, count)) in &per_kind {
        training_repository::insert_kind_score(
            tx,
            &attempt_id,
            &KindScore {
                kind: kind.clone(),
                correct: *correct,
                total: *count,
            },
        )?;
    }
    for entity_id in &snapshot.session.target_entity_ids {
        training_repository::insert_session_entity(tx, &attempt_id, entity_id)?;
    }

    // 5. 更新掌握度 + 复习调度 + 掌握度账本
    //    未作答的题目按 0 分处理（与 TS 的 `answer?.partialScore ?? 0` 一致）
    let entity_kinds = entity_kind_map(tx)?;
    let mut state_cache: HashMap<String, EntityState> = HashMap::new();
    let mut event_seq = 0usize;

    for exercise in &snapshot.session.exercises {
        let partial = partial_by_exercise
            .get(&exercise.id)
            .copied()
            .unwrap_or(0.0);

        for entity_id in &exercise.entity_ids {
            let state = match state_cache.get(entity_id) {
                Some(state) => state.clone(),
                None => {
                    let kind = entity_kinds
                        .get(entity_id)
                        .cloned()
                        .unwrap_or_else(|| "word".to_string());
                    state_repository::find(tx, entity_id)?
                        .unwrap_or_else(|| EntityState::new(entity_id, &kind, &finished_at))
                }
            };

            let outcome = crate::services::state_updater::apply_performance(
                &state,
                partial,
                exercise.difficulty,
                now_ms,
                &finished_at,
            );
            state_repository::upsert(tx, &outcome.next)?;
            state_cache.insert(entity_id.clone(), outcome.next);

            event_seq += 1;
            mastery_repository::insert(
                tx,
                &mastery_repository::MasteryEventInsert {
                    id: &format!("me_{now_ms}_{event_seq}_{entity_id}"),
                    entity_id,
                    kind: &state.kind,
                    mastery_before: outcome.before,
                    mastery_after: outcome.after,
                    source: "training",
                    ref_id: &attempt_id,
                    at: &finished_at,
                },
            )?;
        }
    }

    // 6. 计划项完成标记属于阶段 C5（计划模块），此处不假装已完成

    // 7. 会话标记为已消费，防止二次判分
    training_repository::mark_consumed(tx, session_id)?;

    Ok(ExerciseResult {
        session_id: snapshot.session.id,
        track: snapshot.session.track,
        answers: graded,
        score,
        max_score: total,
        accuracy,
        duration_ms,
        xp,
        per_kind: per_kind
            .into_iter()
            .map(|(kind, (correct, count))| KindScore {
                kind,
                correct,
                total: count,
            })
            .collect(),
        weak_entity_ids: weak_entities.into_iter().collect(),
        finished_at,
        duplicate: false,
    })
}

fn entity_kind_map(conn: &rusqlite::Connection) -> AppResult<HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT id, kind FROM entities WHERE deprecated_at IS NULL")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut map = HashMap::new();
    for row in rows {
        let (id, kind) = row?;
        map.insert(id, kind);
    }
    Ok(map)
}

/// 与会话 id 生成保持一致的短随机后缀（不引入 rand 依赖）
fn create_id(prefix: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{prefix}_{:x}{:06x}",
        now.as_millis(),
        now.subsec_nanos() % 0xFFFFFF
    )
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::repositories::state_repository;
    use serde_json::Value as Json;

    fn db() -> Database {
        let db = Database::open_in_memory_with_content().expect("open");
        db.with_tx(|tx| {
            state_repository::ensure_rows_for_all_entities(tx)?;
            Ok(())
        })
        .expect("seed");
        db
    }

    fn start_spelling(db: &Database) -> TrainingSession {
        db.with_tx(|tx| {
            start(
                tx,
                StartInput {
                    kind: "spelling".to_string(),
                    track: "vocabulary".to_string(),
                    count: Some(3),
                    title: None,
                    difficulty: Some(3.0),
                },
            )
        })
        .expect("start")
    }

    /// 正确答案只存在于服务端快照里；下发给客户端的题目已经被剥离。
    fn correct_answers(db: &Database, session: &TrainingSession) -> Vec<ExerciseAnswer> {
        let full = db
            .with_conn(|conn| training_repository::find_snapshot(conn, &session.id))
            .expect("snapshot")
            .expect("快照存在")
            .session;

        full.exercises
            .iter()
            .map(|exercise| ExerciseAnswer {
                exercise_id: exercise.id.clone(),
                value: exercise.payload().get("answer").cloned().unwrap_or(Json::Null),
                correct: false,       // 故意给错的客户端判定，后端必须重新判分
                partial_score: 0.0,
                elapsed_ms: 1200,
            })
            .collect()
    }

    #[test]
    fn start_persists_snapshot_and_respects_count() {
        let db = db();
        let session = start_spelling(&db);
        assert_eq!(session.exercises.len(), 3);
        assert!(!session.target_entity_ids.is_empty());

        db.with_conn(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM training_session_snapshots WHERE session_id = ?1",
                [session.id.as_str()],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1, "快照必须落库");
            Ok(())
        })
        .expect("snapshot");
    }

    #[test]
    fn submit_grades_on_server_and_updates_state() {
        let db = db();
        let session = start_spelling(&db);
        let answers = correct_answers(&db, &session);

        let result = db
            .with_tx(|tx| submit(tx, &session.id, &answers))
            .expect("submit");

        assert_eq!(result.max_score, 3);
        assert_eq!(result.accuracy, 100);
        assert_eq!(result.score, 3.0);
        assert_eq!(result.answers.len(), 3);
        assert!(result.answers.iter().all(|answer| answer.correct), "服务端应判定为正确");

        // 掌握度必须被写回，且进入复习队列
        db.with_conn(|conn| {
            let mut grew = false;
            for entity_id in &session.target_entity_ids {
                let state = state_repository::find(conn, entity_id)?.expect("状态存在");
                assert!(state.due_at.is_some(), "训练后必须安排复习");
                assert!(state.review_count >= 1);
                if state.mastery > 0.0 {
                    grew = true;
                }
            }
            assert!(grew, "至少一个实体的掌握度应上升");

            let events: i64 = conn.query_row("SELECT COUNT(*) FROM mastery_events", [], |row| row.get(0))?;
            assert!(events > 0);
            Ok(())
        })
        .expect("state");
    }

    #[test]
    fn submit_is_idempotent() {
        let db = db();
        let session = start_spelling(&db);
        let answers = correct_answers(&db, &session);

        let first = db
            .with_tx(|tx| submit(tx, &session.id, &answers))
            .expect("first");
        let second = db
            .with_tx(|tx| submit(tx, &session.id, &answers))
            .expect("second");

        assert_eq!(first.session_id, second.session_id);
        assert_eq!(second.finished_at, first.finished_at, "重复提交应返回既有结果");
        assert!(!first.duplicate, "首次提交不应标记为重复");
        assert!(second.duplicate, "回读既有结果应标记为重复");

        db.with_conn(|conn| {
            let sessions: i64 =
                conn.query_row("SELECT COUNT(*) FROM training_sessions", [], |row| row.get(0))?;
            assert_eq!(sessions, 1, "不能重复计分");
            Ok(())
        })
        .expect("count");
    }

    #[test]
    fn submit_rejects_unknown_session() {
        let db = db();
        let outcome = db.with_tx(|tx| submit(tx, "sess_missing", &[]));
        match outcome {
            Err(err) => assert!(err.to_string().starts_with("not_found:")),
            Ok(_) => panic!("未知会话应报 not_found"),
        }
    }

    #[test]
    fn empty_answers_are_penalised_not_rewarded() {
        let db = db();
        let session = start_spelling(&db);
        let result = db
            .with_tx(|tx| submit(tx, &session.id, &[]))
            .expect("submit");

        assert_eq!(result.accuracy, 0);
        assert_eq!(result.xp, 0);
        assert_eq!(result.answers.len(), 0, "未作答不产生答案行");
        assert_eq!(result.weak_entity_ids.len(), session.target_entity_ids.len());
    }

    #[test]
    fn wrong_answers_do_not_inflate_mastery() {
        let db = db();
        let session = start_spelling(&db);
        let answers: Vec<ExerciseAnswer> = session
            .exercises
            .iter()
            .map(|exercise| ExerciseAnswer {
                exercise_id: exercise.id.clone(),
                value: Json::String("zzz".to_string()),
                correct: true,        // 客户端谎报正确
                partial_score: 1.0,   // 客户端谎报满分
                elapsed_ms: 500,
            })
            .collect();

        let result = db
            .with_tx(|tx| submit(tx, &session.id, &answers))
            .expect("submit");

        assert_eq!(result.accuracy, 0, "服务端判分不采信客户端结果");
        assert!(result.answers.iter().all(|answer| !answer.correct));
    }
}
