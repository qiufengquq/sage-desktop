//! 一次"作答表现"对学习态的影响：掌握度 + SM-2 调度。
//!
//! 训练、测评、复习三条写路径共用这份逻辑，保证"同一个得分在任何入口都得到同样的状态迁移"。

use crate::algorithms::{mastery, srs};
use crate::models::state::EntityState;

pub struct PerformanceOutcome {
    pub next: EntityState,
    pub before: f64,
    pub after: f64,
}

pub fn apply_performance(
    state: &EntityState,
    partial: f64,
    difficulty: i64,
    now_ms: i64,
    at: &str,
) -> PerformanceOutcome {
    let update = update_snapshot(state, partial, difficulty, now_ms);
    let schedule = srs::schedule_next(
        scheduler_state(state),
        srs::rating_for_partial(partial),
        now_ms,
    );
    compose(state, update, schedule, at)
}

/// 复习自评：调度器直接用用户自评，而不是从得分反推评级。
///
/// 掌握度仍按自评映射出的表现分推进（与 `learning-db.ts` 的 `partialOf` 一致），
/// 这样"忘记/模糊/认识/很熟"四种自评在同一条公式上可解释。
pub fn apply_review(
    state: &EntityState,
    rating: srs::Rating,
    partial: f64,
    difficulty: i64,
    now_ms: i64,
    at: &str,
) -> PerformanceOutcome {
    let update = update_snapshot(state, partial, difficulty, now_ms);
    let schedule = srs::schedule_next(scheduler_state(state), rating, now_ms);
    compose(state, update, schedule, at)
}

fn update_snapshot(state: &EntityState, partial: f64, difficulty: i64, now_ms: i64) -> mastery::Update {
    mastery::update_mastery(
        &mastery::Snapshot {
            mastery: state.mastery,
            strength: state.strength,
            state: state.state.clone(),
            review_count: state.review_count,
            lapse_count: state.lapse_count,
            last_reviewed_at: state.last_reviewed_at.clone(),
        },
        partial,
        difficulty as f64,
        now_ms,
    )
}

fn scheduler_state(state: &EntityState) -> srs::SchedulerState {
    srs::SchedulerState {
        ease: state.ease,
        interval_days: state.interval_days,
        repetition: state.repetition,
        lapse_count: state.lapse_count,
    }
}

fn compose(
    state: &EntityState,
    update: mastery::Update,
    schedule: srs::ScheduleOutcome,
    at: &str,
) -> PerformanceOutcome {
    PerformanceOutcome {
        next: EntityState {
            entity_id: state.entity_id.clone(),
            kind: state.kind.clone(),
            mastery: update.next,
            strength: update.strength,
            state: update.state.clone(),
            review_count: update.review_count,
            lapse_count: schedule.lapse_count,
            ease: schedule.ease,
            repetition: schedule.repetition,
            interval_days: schedule.interval_days,
            due_at: Some(schedule.due_at.clone()),
            last_reviewed_at: Some(at.to_string()),
            updated_at: at.to_string(),
        },
        before: update.previous,
        after: update.next,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn improves_mastery_and_schedules_review() {
        let state = EntityState::new("w_demo", "word", "2026-11-13T00:00:00.000Z");
        let outcome = apply_performance(&state, 1.0, 3, 1_763_000_000_000, "2026-11-14T00:00:00.000Z");

        assert!(outcome.after > outcome.before);
        assert!(outcome.next.due_at.is_some());
        assert_eq!(outcome.next.review_count, 1);
        assert_eq!(outcome.next.last_reviewed_at.as_deref(), Some("2026-11-14T00:00:00.000Z"));
    }

    #[test]
    fn same_score_produces_same_state_for_any_entry() {
        let state = EntityState::new("w_demo", "word", "2026-11-13T00:00:00.000Z");
        let first = apply_performance(&state, 0.5, 4, 1_763_000_000_000, "2026-11-14T00:00:00.000Z");
        let second = apply_performance(&state, 0.5, 4, 1_763_000_000_000, "2026-11-14T00:00:00.000Z");

        assert_eq!(first.next.mastery, second.next.mastery);
        assert_eq!(first.next.due_at, second.next.due_at);
        assert_eq!(first.next.ease, second.next.ease);
    }
}
