//! 复习闭环：领取到期队列与提交自评。
//!
//! 提交是一次完整事务，边界如下：
//!   幂等检查 → 读学习态 → 掌握度更新 → SM-2 重新排期
//!   → 写 `review_logs` → 写 `mastery_events` → 覆盖 `entity_states` → COMMIT
//!
//! 队列侧只读：内容来自内容表，调度预览由 `srs` 现算，前端不需要再做派生。

use chrono::{DateTime, Local, TimeZone, Utc};
use rusqlite::{Connection, Transaction};

use crate::algorithms::{srs, xp};
use crate::error::{AppError, AppResult};
use crate::models::knowledge::EntityContent;
use crate::models::review::{
    ReviewCard, ReviewPreview, ReviewQueueItem, ReviewQueuePayload, ReviewSubmitResult,
};
use crate::models::state::EntityState;
use crate::repositories::{entity_repository, mastery_repository, review_repository, state_repository};
use crate::services::state_updater;

/// 自评 → 判分表现（与 `learning-db.ts` 的 `partialOf` 一致）
fn partial_for(rating: srs::Rating) -> f64 {
    match rating {
        srs::Rating::Again => 0.0,
        srs::Rating::Hard => 0.55,
        srs::Rating::Good => 0.9,
        srs::Rating::Easy => 1.0,
    }
}

/// 与 `review.service.ts` 的 `RATING_EXPLANATION` 保持同一套文案
fn explanation_for(rating: srs::Rating) -> &'static str {
    match rating {
        srs::Rating::Again => "标记为忘记：回到重学阶段，10 分钟后再出现，掌握度明显下调。",
        srs::Rating::Hard => "标记为模糊：间隔小幅延长，掌握度小幅提升。",
        srs::Rating::Good => "标记为认识：间隔按难度因子延长，掌握度提升。",
        srs::Rating::Easy => "标记为很熟：间隔延长更多，掌握度提升最多。",
    }
}

/* ---------------------------------- 队列 ---------------------------------- */

pub fn queue(conn: &Connection, limit: i64) -> AppResult<ReviewQueuePayload> {
    let limit = limit.clamp(1, 200);
    let now_ms = Utc::now().timestamp_millis();
    let now_at = srs::iso_from_ms(now_ms);

    let due_count = state_repository::count_due(conn, &now_at)?;
    let mut cards = Vec::new();
    for state in state_repository::due(conn, &now_at, limit)? {
        if let Some(card) = build_card(conn, &state, now_ms, &now_at)? {
            cards.push(card);
        }
    }

    Ok(ReviewQueuePayload { due_count, cards })
}

fn build_card(
    conn: &Connection,
    state: &EntityState,
    now_ms: i64,
    now_at: &str,
) -> AppResult<Option<ReviewCard>> {
    let Some(content) = content_of(conn, &state.entity_id)? else {
        return Ok(None);
    };

    let scheduler = srs::SchedulerState {
        ease: state.ease,
        interval_days: state.interval_days,
        repetition: state.repetition,
        lapse_count: state.lapse_count,
    };
    let preview = ReviewPreview {
        again: preview_label(srs::schedule_next(scheduler, srs::Rating::Again, now_ms)),
        hard: preview_label(srs::schedule_next(scheduler, srs::Rating::Hard, now_ms)),
        good: preview_label(srs::schedule_next(scheduler, srs::Rating::Good, now_ms)),
        easy: preview_label(srs::schedule_next(scheduler, srs::Rating::Easy, now_ms)),
    };

    Ok(Some(ReviewCard {
        item: ReviewQueueItem {
            entity_id: state.entity_id.clone(),
            kind: state.kind.clone(),
            label: content.label,
            secondary: content.secondary,
            difficulty: content.difficulty,
            mastery: state.mastery,
            strength: state.strength,
            state: state.state.clone(),
            due_at: state.due_at.clone().unwrap_or_else(|| now_at.to_string()),
            overdue_days: overdue_days(state.due_at.as_deref(), now_ms),
            last_reviewed_at: state.last_reviewed_at.clone(),
            review_count: state.review_count,
        },
        prompt: content.prompt,
        answer: content.answer,
        detail: content.detail,
        focus: format!(
            "当前掌握度 {} 分 · 已复习 {} 次",
            state.mastery.round(),
            state.review_count
        ),
        preview,
    }))
}

fn preview_label(outcome: srs::ScheduleOutcome) -> String {
    if outcome.interval_days == 0 {
        format!("{} 分钟后", srs::RELEARN_MINUTES)
    } else {
        format!("{} 天后", outcome.interval_days)
    }
}

/// 逾期自然日数（按本机时区的日界，与前端 `overdueDaysOf` 口径一致）
fn overdue_days(due_at: Option<&str>, now_ms: i64) -> i64 {
    let Some(raw) = due_at else {
        return 0;
    };
    let Ok(due) = DateTime::parse_from_rfc3339(raw) else {
        return 0;
    };
    let Some(now) = Utc.timestamp_millis_opt(now_ms).single() else {
        return 0;
    };
    (now.with_timezone(&Local).date_naive() - due.with_timezone(&Local).date_naive()).num_days()
}

/* --------------------------------- 卡片内容 -------------------------------- */

struct ReviewContent {
    label: String,
    secondary: String,
    difficulty: i64,
    prompt: String,
    answer: String,
    detail: Vec<String>,
}

fn content_of(conn: &Connection, entity_id: &str) -> AppResult<Option<ReviewContent>> {
    let Some(content) = entity_repository::entity_content(conn, entity_id)? else {
        return Ok(None);
    };

    let content = match content {
        EntityContent::Word(word) => {
            let secondary = match word.meanings.first() {
                Some(meaning) => format!("{} · {}", word.phonetic, meaning.definition),
                None => word.phonetic.clone(),
            };
            let answer = word
                .meanings
                .iter()
                .map(|meaning| format!("{} {}", pos_label(&meaning.pos), meaning.definition))
                .collect::<Vec<_>>()
                .join("；");
            let mut detail: Vec<String> = word
                .examples
                .iter()
                .take(2)
                .map(|example| format!("{} —— {}", example.en, example.zh))
                .collect();
            if !word.contexts.is_empty() {
                detail.push(format!("使用场景：{}", word.contexts.join(" / ")));
            }

            ReviewContent {
                label: word.term.clone(),
                secondary,
                difficulty: word.difficulty,
                prompt: word.term.clone(),
                answer,
                detail,
            }
        }
        EntityContent::Grammar(grammar) => ReviewContent {
            label: grammar.title.clone(),
            secondary: grammar.title_en.clone(),
            difficulty: grammar.difficulty,
            prompt: format!("{}的核心规则是什么？", grammar.title),
            answer: format!("{}\n结构：{}", grammar.summary, grammar.structure),
            detail: grammar
                .examples
                .iter()
                .take(2)
                .map(|example| format!("{} —— {}", example.en, example.zh))
                .collect(),
        },
        EntityContent::Sentence(sentence) => {
            let titles = grammar_titles(conn, &sentence.grammar_ids)?;
            ReviewContent {
                label: sentence.en.clone(),
                secondary: sentence.zh.clone(),
                difficulty: sentence.difficulty,
                prompt: sentence.zh.clone(),
                answer: sentence.en.clone(),
                detail: if titles.is_empty() {
                    Vec::new()
                } else {
                    vec![format!("涉及语法：{}", titles.join(" / "))]
                },
            }
        }
    };

    Ok(Some(content))
}

fn grammar_titles(conn: &Connection, ids: &[String]) -> AppResult<Vec<String>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT title FROM grammar_points WHERE entity_id = ?1")?;
    let mut titles = Vec::new();
    for id in ids {
        let title: Option<String> = stmt.query_row([id], |row| row.get(0)).ok();
        if let Some(title) = title {
            titles.push(title);
        }
    }
    Ok(titles)
}

fn pos_label(pos: &str) -> &'static str {
    match pos {
        "noun" => "名词",
        "verb" => "动词",
        "adjective" => "形容词",
        "adverb" => "副词",
        "preposition" => "介词",
        "conjunction" => "连词",
        "pronoun" => "代词",
        "phrase" => "词组",
        _ => "",
    }
}

/* ---------------------------------- 提交 ---------------------------------- */

pub fn submit(
    tx: &Transaction<'_>,
    entity_id: &str,
    rating_raw: &str,
    submission_id: Option<&str>,
) -> AppResult<ReviewSubmitResult> {
    let entity_id = entity_id.trim();
    if entity_id.is_empty() {
        return Err(AppError::invalid("entityId 不能为空"));
    }

    let rating = srs::Rating::parse(rating_raw.trim())
        .ok_or_else(|| AppError::invalid(format!("未知的复习自评：{rating_raw}")))?;

    let submission_key = submission_id
        .map(str::trim)
        .filter(|value| !value.is_empty());

    // 幂等：同一 submissionId 重复提交返回既有结果，不重复排期、不重复发 XP
    if let Some(key) = submission_key {
        if let Some(existing) = review_repository::find_by_submission(tx, key)? {
            let stored_rating = srs::Rating::parse(&existing.rating).unwrap_or(rating);
            return Ok(ReviewSubmitResult {
                entity_id: existing.entity_id,
                rating: existing.rating,
                mastery_before: existing.mastery_before,
                mastery_after: existing.mastery_after,
                next_due_at: existing.next_due_at,
                interval_days: existing.next_interval_days,
                ease: existing.ease,
                xp: existing.xp,
                explanation: explanation_for(stored_rating).to_string(),
            });
        }
    }

    let now_ms = Utc::now().timestamp_millis();
    let at = srs::iso_from_ms(now_ms);

    let state = match state_repository::find(tx, entity_id)? {
        Some(state) => state,
        None => {
            // 状态行缺失时以实体内容兜底；实体本身不存在才算 not_found
            let kind = entity_repository::entity_kinds(tx)?
                .get(entity_id)
                .cloned()
                .ok_or_else(|| AppError::not_found(format!("知识点 {entity_id}")))?;
            EntityState::new(entity_id, &kind, &at)
        }
    };

    let difficulty = difficulty_of(tx, entity_id)?;
    let outcome = state_updater::apply_review(
        &state,
        rating,
        partial_for(rating),
        difficulty,
        now_ms,
        &at,
    );

    state_repository::upsert(tx, &outcome.next)?;

    let log_id = format!("rl_{now_ms}_{entity_id}");
    let review_xp = xp::xp_for_review(rating);
    let next_due_at = outcome
        .next
        .due_at
        .clone()
        .unwrap_or_else(|| at.clone());

    review_repository::insert(
        tx,
        &review_repository::ReviewLogInsert {
            id: &log_id,
            submission_id: submission_key,
            entity_id,
            rating: rating.as_str(),
            prev_due_at: state.due_at.as_deref(),
            next_due_at: &next_due_at,
            prev_interval_days: state.interval_days,
            next_interval_days: outcome.next.interval_days,
            ease: outcome.next.ease,
            mastery_before: outcome.before,
            mastery_after: outcome.after,
            xp: review_xp,
            at: &at,
        },
    )?;

    mastery_repository::insert(
        tx,
        &mastery_repository::MasteryEventInsert {
            id: &format!("me_{now_ms}_{entity_id}_review"),
            entity_id,
            kind: &state.kind,
            mastery_before: outcome.before,
            mastery_after: outcome.after,
            source: "review",
            ref_id: &log_id,
            at: &at,
        },
    )?;

    Ok(ReviewSubmitResult {
        entity_id: entity_id.to_string(),
        rating: rating.as_str().to_string(),
        mastery_before: outcome.before,
        mastery_after: outcome.after,
        next_due_at,
        interval_days: outcome.next.interval_days,
        ease: outcome.next.ease,
        xp: review_xp,
        explanation: explanation_for(rating).to_string(),
    })
}

fn difficulty_of(conn: &Connection, entity_id: &str) -> AppResult<i64> {
    let difficulty: Option<i64> = conn
        .query_row("SELECT difficulty FROM entities WHERE id = ?1", [entity_id], |row| {
            row.get(0)
        })
        .ok();
    Ok(difficulty.unwrap_or(3).clamp(1, 5))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn db_with_content() -> Database {
        let db = Database::open_in_memory_with_content().expect("open");
        db.with_tx(|tx| {
            state_repository::ensure_rows_for_all_entities(tx)?;
            Ok(())
        })
        .expect("seed");
        db
    }

    fn first_entity(db: &Database, kind: &str) -> String {
        db.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT id FROM entities WHERE kind = ?1 LIMIT 1",
                [kind],
                |row| row.get::<_, String>(0),
            )?)
        })
        .expect("entity")
    }

    fn make_due(db: &Database, entity_id: &str) {
        const DAY_MS: i64 = 24 * 60 * 60 * 1000;
        // 近期复习过（不触发掌握度衰减），但排期已过期，确保"提升"可被观察到
        let now = Utc::now().timestamp_millis();
        db.with_tx(|tx| {
            let mut state = state_repository::find(tx, entity_id)?.expect("状态行");
            state.mastery = 50.0;
            state.strength = 0.5;
            state.state = "reviewing".to_string();
            state.review_count = 2;
            state.ease = 2.5;
            state.repetition = 2;
            state.interval_days = 5;
            state.due_at = Some(srs::iso_from_ms(now - 2 * DAY_MS));
            state.last_reviewed_at = Some(srs::iso_from_ms(now - 3 * DAY_MS));
            state_repository::upsert(tx, &state)?;
            Ok(())
        })
        .expect("due");
    }

    #[test]
    fn queue_returns_due_cards_with_preview() {
        let db = db_with_content();
        let entity_id = first_entity(&db, "word");
        make_due(&db, &entity_id);

        let payload = db
            .with_conn(|conn| queue(conn, 20))
            .expect("queue");

        assert!(payload.due_count >= 1);
        let card = payload
            .cards
            .iter()
            .find(|card| card.item.entity_id == entity_id)
            .expect("到期实体必须出现");
        assert!(!card.prompt.is_empty());
        assert!(!card.answer.is_empty());
        assert!(card.preview.again.contains("分钟"), "again 应回到重学阶段");
        assert!(card.preview.easy.contains("天"));
        assert!(card.item.overdue_days > 0, "2026-01-01 已逾期");
    }

    #[test]
    fn queue_respects_limit_and_ignores_future_items() {
        let db = db_with_content();
        let entity_id = first_entity(&db, "word");
        make_due(&db, &entity_id);

        let payload = db.with_conn(|conn| queue(conn, 1)).expect("queue");
        assert_eq!(payload.cards.len(), 1, "limit 必须生效");

        db.with_tx(|tx| {
            let mut state = state_repository::find(tx, &entity_id)?.expect("状态行");
            state.due_at = Some("2099-01-01T00:00:00.000Z".to_string());
            state_repository::upsert(tx, &state)?;
            Ok(())
        })
        .expect("future");

        let payload = db.with_conn(|conn| queue(conn, 20)).expect("queue");
        assert_eq!(payload.due_count, 0);
        assert!(payload.cards.is_empty());
    }

    #[test]
    fn submit_writes_log_event_and_schedules_next_review() {
        let db = db_with_content();
        let entity_id = first_entity(&db, "word");
        make_due(&db, &entity_id);

        let result = db
            .with_tx(|tx| submit(tx, &entity_id, "good", None))
            .expect("submit");

        assert_eq!(result.rating, "good");
        assert!(result.mastery_after > result.mastery_before, "认识应提升掌握度");
        assert_eq!(result.xp, xp::xp_for_review(srs::Rating::Good));
        assert_ne!(result.next_due_at, "2026-01-01T00:00:00.000Z", "必须重新排期");

        db.with_conn(|conn| {
            let logs: i64 =
                conn.query_row("SELECT COUNT(*) FROM review_logs", [], |row| row.get(0))?;
            assert_eq!(logs, 1);

            let events: i64 = conn.query_row(
                "SELECT COUNT(*) FROM mastery_events WHERE source = 'review'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(events, 1, "掌握度账本必须记一笔复习来源");

            let state = state_repository::find(conn, &entity_id)?.expect("状态行");
            assert_eq!(state.review_count, 3);
            assert_eq!(
                state.due_at.as_deref(),
                Some(result.next_due_at.as_str()),
                "状态表与日志必须同源"
            );
            Ok(())
        })
        .expect("verify");
    }

    #[test]
    fn submit_is_idempotent_on_submission_id() {
        let db = db_with_content();
        let entity_id = first_entity(&db, "word");
        make_due(&db, &entity_id);

        let first = db
            .with_tx(|tx| submit(tx, &entity_id, "easy", Some("sub_1")))
            .expect("first");
        let second = db
            .with_tx(|tx| submit(tx, &entity_id, "easy", Some("sub_1")))
            .expect("second");

        assert_eq!(first.next_due_at, second.next_due_at);
        assert_eq!(first.xp, second.xp);

        db.with_conn(|conn| {
            assert_eq!(review_repository::count(conn)?, 1, "重复提交不得重复记账");
            Ok(())
        })
        .expect("count");
    }

    #[test]
    fn submit_rejects_unknown_rating_and_entity() {
        let db = db_with_content();

        let bad_rating = db.with_tx(|tx| submit(tx, "w_missing", "perfect", None));
        match bad_rating {
            Err(err) => assert!(err.to_string().starts_with("invalid_argument:"), "{err}"),
            Ok(_) => panic!("未知自评应报 invalid_argument"),
        }

        let bad_entity = db.with_tx(|tx| submit(tx, "w_missing", "good", None));
        match bad_entity {
            Err(err) => assert!(err.to_string().starts_with("not_found:"), "{err}"),
            Ok(_) => panic!("不存在的实体应报 not_found"),
        }
    }

    #[test]
    fn again_resets_repetition_and_returns_in_ten_minutes() {
        let db = db_with_content();
        let entity_id = first_entity(&db, "word");
        make_due(&db, &entity_id);

        let result = db
            .with_tx(|tx| submit(tx, &entity_id, "again", None))
            .expect("submit");

        assert_eq!(result.interval_days, 0, "忘记后回到重学阶段");
        assert!(result.mastery_after < result.mastery_before, "忘记必须下调掌握度");

        db.with_conn(|conn| {
            let state = state_repository::find(conn, &entity_id)?.expect("状态行");
            assert_eq!(state.repetition, 0);
            assert_eq!(state.lapse_count, 1);
            Ok(())
        })
        .expect("verify");
    }
}
