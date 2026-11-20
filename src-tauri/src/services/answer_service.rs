//! 逐题即时反馈。
//!
//! 判分权收归后端后的配套能力：客户端只提交 `{exerciseId, value}`，
//! 由服务端用**库内题目**判分并回传可读答案与讲解。
//!
//! 两条约束：
//! 1. 只能对"本次会话/本次测评里真实存在的题目"发问，防止拿题库当答案查询器。
//! 2. 判分与最终提交共用 `grading::score_answer`，不允许出现两套口径。

use rusqlite::Connection;
use serde_json::Value as Json;

use crate::algorithms::grading::{answer_of, score_answer};
use crate::error::{AppError, AppResult};
use crate::models::exercise::{AnswerCheck, Exercise};
use crate::repositories::{assessment_repository, training_repository};

fn build_check(exercise: &Exercise, value: &Json) -> AnswerCheck {
    let outcome = score_answer(&exercise.kind, exercise.payload(), value);
    AnswerCheck {
        exercise_id: exercise.id.clone(),
        correct: outcome.correct,
        partial_score: outcome.partial,
        correct_answer: answer_of(&exercise.kind, exercise.payload()),
        explanation: exercise.explanation.clone(),
    }
}

fn find_in<'a>(exercises: &'a [Exercise], exercise_id: &str) -> AppResult<&'a Exercise> {
    exercises
        .iter()
        .find(|exercise| exercise.id == exercise_id)
        .ok_or_else(|| AppError::not_found(format!("题目不在当前任务中：{exercise_id}")))
}

/// 训练中的单题校验：题目必须来自该会话的快照。
pub fn check_training(
    conn: &Connection,
    session_id: &str,
    exercise_id: &str,
    value: &Json,
) -> AppResult<AnswerCheck> {
    let snapshot = training_repository::find_snapshot(conn, session_id)?
        .ok_or_else(|| AppError::not_found(format!("训练会话不存在：{session_id}")))?;

    let exercise = find_in(&snapshot.session.exercises, exercise_id)?;
    Ok(build_check(exercise, value))
}

/// 测评中的单题校验：题目必须属于该模块。
pub fn check_assessment(
    conn: &Connection,
    module_id: &str,
    exercise_id: &str,
    value: &Json,
) -> AppResult<AnswerCheck> {
    let module = assessment_repository::find_module(conn, module_id)?
        .ok_or_else(|| AppError::not_found(format!("测评模块不存在：{module_id}")))?;

    let exercises: Vec<Exercise> = module
        .sections
        .iter()
        .flat_map(|section| section.items.iter().cloned())
        .collect();

    let exercise = find_in(&exercises, exercise_id)?;
    Ok(build_check(exercise, value))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::database::Database;
    use crate::services::training_service::{self, StartInput};

    fn db() -> Database {
        Database::open_in_memory_with_content().expect("内存库")
    }

    fn start_session(db: &Database) -> crate::models::exercise::TrainingSession {
        db.with_tx(|tx| {
            training_service::start(
                tx,
                StartInput {
                    kind: "multiple-choice".into(),
                    track: "vocabulary".into(),
                    count: Some(3),
                    title: None,
                    difficulty: Some(3.0),
                },
            )
        })
        .expect("start")
    }

    #[test]
    fn training_check_grades_with_server_side_key() {
        let db = db();
        let session = start_session(&db);

        // 下发形态不含答案，正确答案只能从服务端快照读取（这正是本次改动的目的）
        let first = session.exercises.first().cloned().expect("有题");
        assert!(
            first.payload().get("options").and_then(|v| v.as_array()).is_some_and(|options| {
                options.iter().all(|option| option.get("correct").is_none())
            }),
            "下发题目不应包含 correct 标记"
        );

        let full = db
            .with_conn(|conn| training_repository::find_snapshot(conn, &session.id))
            .expect("snapshot")
            .expect("快照存在")
            .session;
        let correct_label = full
            .exercises
            .iter()
            .find(|exercise| exercise.id == first.id)
            .and_then(|exercise| exercise.payload().get("options").cloned())
            .and_then(|options| {
                options
                    .as_array()
                    .and_then(|items| {
                        items
                            .iter()
                            .find(|option| option.get("correct").and_then(|v| v.as_bool()) == Some(true))
                    })
                    .and_then(|option| option.get("id").cloned())
            })
            .expect("有正确选项");

        let check = db
            .with_conn(|conn| check_training(conn, &session.id, &first.id, &correct_label))
            .expect("check");
        assert!(check.correct, "选对正确答案应判对");
        assert_eq!(check.partial_score, 1.0);
        assert!(!check.correct_answer.is_null(), "必须回传可读答案");

        let wrong = db
            .with_conn(|conn| check_training(conn, &session.id, &first.id, &json!("__nope__")))
            .expect("check wrong");
        assert!(!wrong.correct);
    }

    #[test]
    fn training_check_rejects_foreign_exercise() {
        let db = db();
        let session = start_session(&db);

        let outcome = db.with_conn(|conn| {
            check_training(conn, &session.id, "ex_not_in_session", &json!("a"))
        });
        assert!(outcome.is_err(), "不允许对会话外的题目发问");
    }

    fn assert_no_answer_keys(kind: &str, exercise: &Exercise) {
        let payload = exercise.payload();
        assert!(payload.get("answer").is_none(), "{kind} 下发了 answer");
        assert!(payload.get("pairs").is_none(), "{kind} 下发了 pairs");
        assert!(payload.get("references").is_none(), "{kind} 下发了 references");

        if let Some(options) = payload.get("options").and_then(Json::as_array) {
            assert!(
                options.iter().all(|option| option.get("correct").is_none()),
                "{kind} 下发了 correct 标记"
            );
        }
        if let Some(turns) = payload.get("turns").and_then(Json::as_array) {
            assert!(
                turns.iter().all(|turn| turn.get("expected").is_none()),
                "{kind} 下发了 expected"
            );
        }
    }

    #[test]
    fn player_view_strips_every_answer_key() {
        let db = db();
        for kind in [
            "spelling",
            "dictation",
            "multiple-choice",
            "cloze",
            "translate",
            "rewrite",
            "error-correction",
            "sentence-order",
            "word-match",
            "roleplay",
            "shadowing",
        ] {
            let Ok(session) = db.with_tx(|tx| {
                training_service::start(
                    tx,
                    StartInput {
                        kind: kind.into(),
                        track: "vocabulary".into(),
                        count: Some(2),
                        title: None,
                        difficulty: Some(3.0),
                    },
                )
            }) else {
                continue; // 该题型不在词汇 track 里
            };

            for exercise in &session.exercises {
                assert_no_answer_keys(kind, exercise);
            }
        }
    }

    #[test]
    fn assessment_modules_strip_answer_keys() {
        let db = db();
        // 注意：必须走 service 层（仓储返回的是"判分用的完整题目"）
        let modules = db
            .with_conn(crate::services::assessment_service::modules)
            .expect("modules");
        assert!(!modules.is_empty());
        for module in &modules {
            for section in &module.sections {
                for exercise in &section.items {
                    assert_no_answer_keys(&exercise.kind, exercise);
                }
            }
        }
    }

    #[test]
    fn assessment_check_only_accepts_module_items() {
        let db = db();
        let module = db
            .with_conn(|conn| assessment_repository::find_module(conn, "vocabulary"))
            .expect("find")
            .expect("存在");
        let item = module
            .sections
            .iter()
            .flat_map(|section| section.items.iter())
            .next()
            .cloned()
            .expect("有题");

        let check = db
            .with_conn(|conn| check_assessment(conn, "vocabulary", &item.id, &json!("")))
            .expect("check");
        assert_eq!(check.exercise_id, item.id);

        let foreign = db.with_conn(|conn| check_assessment(conn, "grammar", &item.id, &json!("")));
        assert!(foreign.is_err(), "跨模块提问应被拒绝");
    }
}
