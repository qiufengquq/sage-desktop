//! 测评服务：模块列表与提交。
//!
//! 提交同样是一个事务：判分 → 写测评事件（含分项成绩与逐题作答）→ 更新掌握度与调度
//! → 写掌握度账本。计划重排属于阶段 C5，这里只把画像所需数据写全。

use std::collections::HashMap;

use chrono::Utc;
use rusqlite::Transaction;

use crate::algorithms::{ability, grading};
use crate::database::now_iso;
use crate::error::{AppError, AppResult};
use crate::models::assessment::{
    AssessmentModule, AssessmentOutcome, AssessmentSection, FollowUp, SectionOutcome,
};
use crate::models::exercise::{Exercise, ExerciseAnswer};
use crate::models::state::EntityState;
use crate::repositories::{
    assessment_repository, mastery_repository, state_repository,
};

/// 模块结构（含题目），下发给客户端前**剥离答案字段**。
///
/// 拆分点很关键：仓储返回的是"判分用的完整题目"，服务层负责转成"作答用的题目"，
/// 这样提交与逐题校验仍能从库里读到正确答案。
pub fn modules(conn: &rusqlite::Connection) -> AppResult<Vec<AssessmentModule>> {
    let modules = assessment_repository::modules(conn)?;
    Ok(modules
        .into_iter()
        .map(|module| AssessmentModule {
            sections: module
                .sections
                .into_iter()
                .map(|section| AssessmentSection {
                    items: section.items.iter().map(Exercise::to_player).collect(),
                    ..section
                })
                .collect(),
            ..module
        })
        .collect())
}

pub fn submit(
    tx: &Transaction<'_>,
    module_id: &str,
    answers: &[ExerciseAnswer],
    submission_id: &str,
) -> AppResult<AssessmentOutcome> {
    // 幂等键必填：由客户端生成的 UUID。禁止用 "{module}_{now}" 之类的派生键，
    // 那种键既不可控也不幂等（重复提交会重复记账）。
    if submission_id.trim().is_empty() {
        return Err(AppError::invalid("submissionId 不能为空"));
    }
    let submission_key = submission_id.to_string();

    if let Some(existing) = assessment_repository::find_outcome_by_submission(tx, &submission_key)? {
        return Ok(AssessmentOutcome {
            duplicate: true,
            ..existing
        });
    }

    let module = assessment_repository::find_module(tx, module_id)?
        .ok_or_else(|| AppError::not_found(format!("测评模块 {module_id}")))?;

    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    let finished_at = now_iso();
    let answer_by_id: HashMap<&str, &ExerciseAnswer> = answers
        .iter()
        .map(|answer| (answer.exercise_id.as_str(), answer))
        .collect();

    // 1. 按分项判分
    let mut per_section: Vec<SectionOutcome> = Vec::with_capacity(module.sections.len());
    let mut partial_by_exercise: HashMap<String, f64> = HashMap::new();
    let mut graded: Vec<ExerciseAnswer> = Vec::new();

    for section in &module.sections {
        let mut correct = 0i64;
        let mut partial_sum = 0.0f64;

        for item in &section.items {
            let answer = answer_by_id.get(item.id.as_str());
            let outcome = match answer {
                Some(answer) => grading::score_answer(&item.kind, item.payload(), &answer.value),
                None => grading::ScoreOutcome { correct: false, partial: 0.0 },
            };

            partial_by_exercise.insert(item.id.clone(), outcome.partial);
            partial_sum += outcome.partial;
            if outcome.correct {
                correct += 1;
            }

            if let Some(answer) = answer {
                graded.push(ExerciseAnswer {
                    exercise_id: item.id.clone(),
                    value: answer.value.clone(),
                    correct: outcome.correct,
                    partial_score: outcome.partial,
                    elapsed_ms: answer.elapsed_ms,
                });
            }
        }

        let total = section.items.len() as i64;
        per_section.push(SectionOutcome {
            section_id: section.id.clone(),
            title: section.title.clone(),
            correct,
            total,
            score: if total == 0 {
                0
            } else {
                ((partial_sum / total as f64) * 100.0).round() as i64
            },
        });
    }

    // 2. 汇总成绩
    let scored: Vec<&SectionOutcome> = per_section.iter().filter(|item| item.total > 0).collect();
    let score = if scored.is_empty() {
        0
    } else {
        (scored.iter().map(|item| item.score).sum::<i64>() as f64 / scored.len() as f64).round()
            as i64
    };

    let mut sorted: Vec<&SectionOutcome> = scored.clone();
    sorted.sort_by(|left, right| right.score.cmp(&left.score));

    let strengths: Vec<String> = sorted
        .iter()
        .take(2)
        .map(|item| format!("{}：{} 分", item.title, item.score))
        .collect();

    let mut tail: Vec<&SectionOutcome> = sorted.iter().rev().take(2).copied().collect();
    tail.reverse();
    let weaknesses: Vec<String> = tail
        .into_iter()
        .filter(|item| item.score < 75)
        .map(|item| format!("{}：{} 分", item.title, item.score))
        .collect();

    let weakest = sorted.last().copied();
    let recommendation = match weakest {
        Some(item) => format!(
            "优先补强「{}」。建议接下来 3 天每天 6 分钟定向训练，再复测一次看趋势。",
            item.title
        ),
        None => "各项水平接近，可以直接进入 B2 阶段的长句与学术表达训练。".to_string(),
    };

    let follow_up = assessment_repository::weakest_follow_up(module_id, &per_section, Some(&module))
        .map(|item| FollowUp {
            label: item.label,
            kind: item.kind,
            track: item.track,
        });

    let level = ability::level_of_score(score).to_string();
    let duration_ms: i64 = answers.iter().map(|answer| answer.elapsed_ms).sum();
    let minutes = ((duration_ms as f64 / 60_000.0).round() as i64).max(1);

    // 3. 写测评事件
    let attempt_id = format!("aa_{now_ms}");
    assessment_repository::insert_attempt(
        tx,
        &assessment_repository::AttemptInsert {
            id: &attempt_id,
            submission_id: &submission_key,
            module_id,
            score,
            accuracy: score,
            level: &level,
            strengths: &strengths,
            weaknesses: &weaknesses,
            recommendation: &recommendation,
            minutes,
            finished_at: &finished_at,
        },
    )?;

    for section in &per_section {
        assessment_repository::insert_section_result(tx, &attempt_id, section)?;
    }
    for answer in &graded {
        assessment_repository::insert_answer(tx, &attempt_id, answer)?;
    }

    // 4. 更新掌握度（测评也计入学习表现）
    let mut state_cache: HashMap<String, EntityState> = HashMap::new();
    let mut event_seq = 0usize;

    for section in &module.sections {
        for item in &section.items {
            let partial = partial_by_exercise.get(&item.id).copied().unwrap_or(0.0);

            for entity_id in &item.entity_ids {
                let state = match state_cache.get(entity_id) {
                    Some(state) => state.clone(),
                    None => state_repository::find(tx, entity_id)?.unwrap_or_else(|| {
                        let kind = entity_kind(tx, entity_id).unwrap_or_else(|| "word".to_string());
                        EntityState::new(entity_id, &kind, &finished_at)
                    }),
                };

                let outcome = crate::services::state_updater::apply_performance(
                    &state,
                    partial,
                    item.difficulty,
                    now_ms,
                    &finished_at,
                );
                state_repository::upsert(tx, &outcome.next)?;
                state_cache.insert(entity_id.clone(), outcome.next);

                event_seq += 1;
                mastery_repository::insert(
                    tx,
                    &mastery_repository::MasteryEventInsert {
                        id: &format!("me_{now_ms}_a{event_seq}_{entity_id}"),
                        entity_id,
                        kind: &state.kind,
                        mastery_before: outcome.before,
                        mastery_after: outcome.after,
                        source: "assessment",
                        ref_id: &attempt_id,
                        at: &finished_at,
                    },
                )?;
            }
        }
    }

    Ok(AssessmentOutcome {
        module_id: module_id.to_string(),
        score,
        accuracy: score,
        level,
        per_section,
        strengths,
        weaknesses,
        recommendation,
        finished_at,
        follow_up,
        duplicate: false,
    })
}

fn entity_kind(conn: &rusqlite::Connection, entity_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT kind FROM entities WHERE id = ?1",
        [entity_id],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
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

    fn perfect_answers(module: &AssessmentModule) -> Vec<ExerciseAnswer> {
        module
            .sections
            .iter()
            .flat_map(|section| section.items.iter())
            .map(|item| ExerciseAnswer {
                exercise_id: item.id.clone(),
                value: item
                    .payload()
                    .get("answer")
                    .cloned()
                    .or_else(|| {
                        item.payload()
                            .get("options")
                            .and_then(|options| options.as_array())
                            .and_then(|options| {
                                options
                                    .iter()
                                    .find(|option| {
                                        option.get("correct").and_then(Json::as_bool) == Some(true)
                                    })
                                    .and_then(|option| option.get("id").cloned())
                            })
                    })
                    .unwrap_or(Json::Null),
                correct: false,
                partial_score: 0.0,
                elapsed_ms: 2000,
            })
            .collect()
    }

    #[test]
    fn modules_carry_sections_and_items() {
        let db = db();
        let modules = db.with_conn(modules).expect("modules");
        assert_eq!(modules.len(), 3);
        assert!(modules.iter().all(|module| !module.sections.is_empty()));

        // 允许存在"无需作答"的分项（如口语评估项），但每个模块至少要有一道题
        for module in &modules {
            let items: usize = module.sections.iter().map(|section| section.items.len()).sum();
            assert!(items > 0, "模块 {} 没有任何题目", module.id);
        }
    }

    #[test]
    fn submit_writes_attempt_sections_and_answers() {
        let db = db();
        let module = db
            .with_conn(|conn| assessment_repository::find_module(conn, "vocabulary"))
            .expect("find")
            .expect("存在");
        let answers = perfect_answers(&module);

        let outcome = db
            .with_tx(|tx| submit(tx, "vocabulary", &answers, "sub_test_1"))
            .expect("submit");

        assert_eq!(outcome.per_section.len(), module.sections.len());
        assert!(outcome.per_section.iter().all(|section| section.score > 0));
        assert!(outcome.level.len() == 2);

        db.with_conn(|conn| {
            let attempts: i64 =
                conn.query_row("SELECT COUNT(*) FROM assessment_attempts", [], |row| row.get(0))?;
            let sections: i64 = conn.query_row(
                "SELECT COUNT(*) FROM assessment_section_results",
                [],
                |row| row.get(0),
            )?;
            let answers_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM assessment_answers", [], |row| row.get(0))?;
            assert_eq!(attempts, 1);
            assert_eq!(sections, module.sections.len() as i64);
            assert!(answers_count > 0);
            Ok(())
        })
        .expect("rows");
    }

    #[test]
    fn submit_is_idempotent_by_submission_id() {
        let db = db();
        let module = db
            .with_conn(|conn| assessment_repository::find_module(conn, "vocabulary"))
            .expect("find")
            .expect("存在");
        let answers = perfect_answers(&module);

        let first = db
            .with_tx(|tx| submit(tx, "vocabulary", &answers, "sub_fixed"))
            .expect("first");
        let second = db
            .with_tx(|tx| submit(tx, "vocabulary", &answers, "sub_fixed"))
            .expect("second");

        assert_eq!(first.finished_at, second.finished_at);
        assert!(!first.duplicate, "首次提交不应标记为重复");
        assert!(second.duplicate, "命中幂等键应标记为重复");
        db.with_conn(|conn| {
            let attempts: i64 =
                conn.query_row("SELECT COUNT(*) FROM assessment_attempts", [], |row| row.get(0))?;
            assert_eq!(attempts, 1);
            Ok(())
        })
        .expect("count");
    }

    #[test]
    fn submit_requires_submission_id() {
        let db = db();
        let module = db
            .with_conn(|conn| assessment_repository::find_module(conn, "vocabulary"))
            .expect("find")
            .expect("存在");
        let answers = perfect_answers(&module);

        let outcome = db.with_tx(|tx| submit(tx, "vocabulary", &answers, "   "));
        match outcome {
            Err(err) => assert!(
                err.to_string().starts_with("invalid_argument:"),
                "空幂等键应报 invalid_argument：{err}"
            ),
            Ok(_) => panic!("空 submissionId 不应被接受"),
        }
    }

    #[test]
    fn module_last_attempt_is_refreshed() {
        let db = db();
        let module = db
            .with_conn(|conn| assessment_repository::find_module(conn, "grammar"))
            .expect("find")
            .expect("存在");
        let answers = perfect_answers(&module);

        db.with_tx(|tx| submit(tx, "grammar", &answers, "sub_last"))
            .expect("submit");

        let modules = db.with_conn(modules).expect("modules");
        let grammar = modules
            .iter()
            .find(|item| item.id == "grammar")
            .expect("grammar");
        assert!(grammar.last_attempt.is_some(), "最近一次成绩应来自真实记录");
    }

    #[test]
    fn unknown_module_is_not_found() {
        let db = db();
        let outcome = db.with_tx(|tx| submit(tx, "nope", &[], "sub_unknown"));
        match outcome {
            Err(err) => assert!(err.to_string().starts_with("not_found:")),
            Ok(_) => panic!("未知模块应报 not_found"),
        }
    }
}
