//! 掌握度更新（对照组：`src/lib/algorithms/mastery.ts`）。
//!
//! 目标与 TS 完全一致：不覆盖写、先衰减再逼近、难度参与学习率、每次更新可解释。
//! 公式：
//!   decayed  = mastery × 衰减系数
//!   target   = 答对 ? 70 + 30 × 得分率 : 40 × 得分率
//!   learning = clamp(0.10 + 0.035 × 难度 + 新手加成 − 老手折扣, 0.06, 0.30)
//!   next     = decayed + learning × (target − decayed)

use chrono::DateTime;

use super::util::{clamp, round1, round3};

pub const CORRECT_THRESHOLD: f64 = 0.7;
pub const FAILURE_THRESHOLD: f64 = 0.35;
pub const DECAY_AFTER_DAYS: f64 = 7.0;
pub const DECAY_PER_DAY: f64 = 0.6;
pub const MAX_DECAY_RATIO: f64 = 0.5;
pub const STRENGTH_DECAY_AFTER_DAYS: f64 = 14.0;
pub const STRENGTH_DECAY_PER_WEEK: f64 = 0.12;
pub const MASTERED_AT: f64 = 80.0;
pub const MASTERED_REVIEWS: i64 = 3;
pub const REVIEWING_AT: f64 = 40.0;

const DAY_MS: f64 = 24.0 * 60.0 * 60.0 * 1000.0;

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub mastery: f64,
    pub strength: f64,
    pub state: String,
    pub review_count: i64,
    pub lapse_count: i64,
    pub last_reviewed_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Update {
    pub previous: f64,
    pub next: f64,
    pub delta: f64,
    pub strength: f64,
    pub state: String,
    pub review_count: i64,
    pub lapse_count: i64,
    pub lapsed: bool,
}

fn days_since(iso: Option<&str>, now_ms: i64) -> Option<f64> {
    let iso = iso?;
    let time = DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|value| value.timestamp_millis())
        .or_else(|| iso.parse::<i64>().ok())?;
    let diff = (now_ms - time) as f64;
    Some((diff / DAY_MS).max(0.0))
}

pub fn decay_mastery(snapshot: &Snapshot, now_ms: i64) -> f64 {
    let Some(days) = days_since(snapshot.last_reviewed_at.as_deref(), now_ms) else {
        return snapshot.mastery;
    };
    if days <= DECAY_AFTER_DAYS {
        return snapshot.mastery;
    }
    let overdue = days - DECAY_AFTER_DAYS;
    let penalty = (snapshot.mastery * MAX_DECAY_RATIO).min(overdue * DECAY_PER_DAY);
    clamp(snapshot.mastery - penalty, 0.0, 100.0)
}

pub fn decay_strength(snapshot: &Snapshot, now_ms: i64) -> f64 {
    let Some(days) = days_since(snapshot.last_reviewed_at.as_deref(), now_ms) else {
        return snapshot.strength;
    };
    if days <= STRENGTH_DECAY_AFTER_DAYS {
        return snapshot.strength;
    }
    let weeks = (days - STRENGTH_DECAY_AFTER_DAYS) / 7.0;
    let keep = (1.0 - weeks * STRENGTH_DECAY_PER_WEEK).max(0.2);
    clamp(snapshot.strength * keep, 0.0, 1.0)
}

fn learning_rate(snapshot: &Snapshot, difficulty: f64) -> f64 {
    let novelty = if snapshot.review_count < 2 {
        0.06
    } else if snapshot.review_count > 8 {
        -0.04
    } else {
        0.0
    };
    clamp(0.1 + 0.035 * difficulty + novelty, 0.06, 0.3)
}

fn next_state(mastery: f64, review_count: i64, lapsed: bool) -> String {
    if lapsed {
        return "lapsed".to_string();
    }
    if mastery >= MASTERED_AT && review_count >= MASTERED_REVIEWS {
        return "mastered".to_string();
    }
    if mastery >= REVIEWING_AT {
        return "reviewing".to_string();
    }
    if mastery > 0.0 {
        return "learning".to_string();
    }
    "new".to_string()
}

pub fn update_mastery(snapshot: &Snapshot, partial_score: f64, difficulty: f64, now_ms: i64) -> Update {
    let partial = clamp(partial_score, 0.0, 1.0);
    let correct = partial >= CORRECT_THRESHOLD;

    let decayed = decay_mastery(snapshot, now_ms);
    let base_target = if correct {
        70.0 + 30.0 * partial
    } else {
        40.0 * partial
    };
    let difficulty_impact = if correct { (difficulty - 3.0) * 2.0 } else { 0.0 };
    let target = clamp(base_target + difficulty_impact, 0.0, 100.0);

    let rate = learning_rate(snapshot, difficulty);
    let next = clamp(decayed + rate * (target - decayed), 0.0, 100.0);

    let review_count = snapshot.review_count + 1;
    let lapsed = partial < FAILURE_THRESHOLD
        && (snapshot.state == "mastered" || snapshot.mastery >= 70.0);
    let lapse_count = snapshot.lapse_count + i64::from(partial < FAILURE_THRESHOLD);

    let decayed_strength = decay_strength(snapshot, now_ms);
    let strength = clamp(decayed_strength * 0.7 + partial * 0.3, 0.0, 1.0);

    Update {
        previous: round1(snapshot.mastery),
        next: round1(next),
        delta: round1(next - snapshot.mastery),
        strength: round3(strength),
        state: next_state(next, review_count, lapsed),
        review_count,
        lapse_count,
        lapsed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(mastery: f64) -> Snapshot {
        Snapshot {
            mastery,
            strength: 0.4,
            state: "learning".to_string(),
            review_count: 1,
            lapse_count: 0,
            last_reviewed_at: Some("2026-11-13T00:00:00.000Z".to_string()),
        }
    }

    #[test]
    fn correct_answer_moves_towards_target() {
        let update = update_mastery(&snapshot(20.0), 1.0, 3.0, 1_797_000_000_000);
        assert!(update.next > 20.0);
        assert!(update.next < 100.0, "一次答对不应直接满分");
        assert_eq!(update.review_count, 2);
        assert!(!update.lapsed);
    }

    #[test]
    fn wrong_answer_can_lapse_a_mastered_item() {
        let mut state = snapshot(85.0);
        state.state = "mastered".to_string();
        let update = update_mastery(&state, 0.0, 3.0, 1_797_000_000_000);
        assert!(update.lapsed, "已掌握条目答错应判定为遗忘");
        assert_eq!(update.state, "lapsed");
        assert_eq!(update.lapse_count, 1);
        assert!(update.next < 85.0);
    }

    #[test]
    fn stale_items_decay_before_scoring() {
        let mut state = snapshot(80.0);
        state.last_reviewed_at = Some("2026-09-01T00:00:00.000Z".to_string());
        let now = 1_797_000_000_000; // 2026-12 左右
        let decayed = decay_mastery(&state, now);
        assert!(decayed < 80.0, "超过 7 天应开始衰减");
        assert!(decayed >= 40.0, "单次衰减不超过当前值的一半");
    }

    #[test]
    fn state_thresholds_are_respected() {
        let mut state = snapshot(79.0);
        state.review_count = 5;
        // 距上次复习 1 天：不触发衰减，只验证阈值判定
        let now = 1_763_000_000_000; // 2025-11 之后任意固定值
        state.last_reviewed_at = Some(super::super::srs::iso_from_ms(now - DAY_MS as i64));
        let update = update_mastery(&state, 1.0, 5.0, now);
        assert!(update.next >= MASTERED_AT);
        assert_eq!(update.state, "mastered");
    }
}
