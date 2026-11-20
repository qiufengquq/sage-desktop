//! 复习调度（SM-2 变体，对照组：`src/lib/algorithms/srs.ts`）。
//!
//! 规则：
//!   again：重学阶段，10 分钟后再次出现，EF −0.20，lapse +1
//!   hard ：间隔 ×1.2（最少 1 天），EF −0.15
//!   good ：0→1 天，1→3 天，之后 ×EF
//!   easy ：0→2 天，1→4 天，之后 ×EF×1.3
//!   EF ∈ [1.3, 2.8]，间隔上限 180 天。

use chrono::{SecondsFormat, TimeZone, Utc};

use super::util::clamp;

pub const MIN_EASE: f64 = 1.3;
pub const MAX_EASE: f64 = 2.8;
pub const INITIAL_EASE: f64 = 2.5;
pub const MAX_INTERVAL_DAYS: i64 = 180;
pub const RELEARN_MINUTES: i64 = 10;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const MINUTE_MS: i64 = 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rating {
    Again,
    Hard,
    Good,
    Easy,
}

impl Rating {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "again" => Some(Self::Again),
            "hard" => Some(Self::Hard),
            "good" => Some(Self::Good),
            "easy" => Some(Self::Easy),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Again => "again",
            Self::Hard => "hard",
            Self::Good => "good",
            Self::Easy => "easy",
        }
    }

    fn ease_delta(&self) -> f64 {
        match self {
            Self::Again => -0.2,
            Self::Hard => -0.15,
            Self::Good => 0.0,
            Self::Easy => 0.15,
        }
    }

    fn first_interval(&self) -> i64 {
        match self {
            Self::Again => 0,
            Self::Hard => 1,
            Self::Good => 1,
            Self::Easy => 2,
        }
    }

    fn second_interval(&self) -> i64 {
        match self {
            Self::Again => 0,
            Self::Hard => 2,
            Self::Good => 3,
            Self::Easy => 4,
        }
    }
}

/// 训练正确率 → 复习自评（与 `learning-db.ts:ratingForPartial` 一致）
pub fn rating_for_partial(partial: f64) -> Rating {
    if partial >= 0.9 {
        Rating::Good
    } else if partial >= 0.7 {
        Rating::Hard
    } else {
        Rating::Again
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SchedulerState {
    pub ease: f64,
    pub interval_days: i64,
    pub repetition: i64,
    pub lapse_count: i64,
}

#[derive(Debug, Clone)]
pub struct ScheduleOutcome {
    pub ease: f64,
    pub interval_days: i64,
    pub repetition: i64,
    pub lapse_count: i64,
    pub due_at: String,
    pub explanation: String,
}

fn clamp_ease(ease: f64) -> f64 {
    clamp(ease, MIN_EASE, MAX_EASE)
}

/// `now_ms` 显式传入，保证可测试、可回放
pub fn schedule_next(state: SchedulerState, rating: Rating, now_ms: i64) -> ScheduleOutcome {
    let ease = clamp_ease(state.ease + rating.ease_delta());
    let repetition = if rating == Rating::Again {
        0
    } else {
        state.repetition + 1
    };
    let lapse_count = state.lapse_count + i64::from(rating == Rating::Again);

    let (interval_days, explanation) = if rating == Rating::Again {
        (
            0,
            format!("回到重学阶段，{RELEARN_MINUTES} 分钟后再次出现"),
        )
    } else if state.repetition == 0 {
        let days = rating.first_interval();
        (days, format!("首次记住，{days} 天后复习"))
    } else if state.repetition == 1 {
        let days = rating.second_interval();
        (days, format!("第二次成功，间隔延长到 {days} 天"))
    } else {
        let multiplier = match rating {
            Rating::Easy => ease * 1.3,
            Rating::Hard => 1.2,
            _ => ease,
        };
        let days = ((state.interval_days as f64 * multiplier).round() as i64).max(1);
        (
            days,
            format!("按难度因子 {ease:.2} 推算，间隔延长到 {days} 天"),
        )
    };

    let interval_days = interval_days.min(MAX_INTERVAL_DAYS);
    let due_ms = if interval_days == 0 {
        now_ms + RELEARN_MINUTES * MINUTE_MS
    } else {
        now_ms + interval_days * DAY_MS
    };

    ScheduleOutcome {
        ease,
        interval_days,
        repetition,
        lapse_count,
        due_at: iso_from_ms(due_ms),
        explanation,
    }
}

/// 与 `new Date(ms).toISOString()` 输出一致：`YYYY-MM-DDTHH:MM:SS.sssZ`
pub fn iso_from_ms(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(repetition: i64, interval_days: i64) -> SchedulerState {
        SchedulerState {
            ease: INITIAL_EASE,
            interval_days,
            repetition,
            lapse_count: 0,
        }
    }

    #[test]
    fn again_reschedules_in_ten_minutes() {
        let now = 1_797_000_000_000;
        let outcome = schedule_next(state(3, 10), Rating::Again, now);
        assert_eq!(outcome.interval_days, 0);
        assert_eq!(outcome.repetition, 0, "失败后连续成功次数归零");
        assert_eq!(outcome.lapse_count, 1);
        assert_eq!(outcome.ease, INITIAL_EASE - 0.2);
        assert_eq!(outcome.due_at, iso_from_ms(now + 10 * 60 * 1000));
    }

    #[test]
    fn first_and_second_success_use_fixed_ladders() {
        assert_eq!(schedule_next(state(0, 0), Rating::Good, 0).interval_days, 1);
        assert_eq!(schedule_next(state(1, 1), Rating::Good, 0).interval_days, 3);
        assert_eq!(schedule_next(state(0, 0), Rating::Easy, 0).interval_days, 2);
        assert_eq!(schedule_next(state(1, 2), Rating::Easy, 0).interval_days, 4);
    }

    #[test]
    fn subsequent_intervals_multiply_by_ease() {
        let outcome = schedule_next(state(3, 10), Rating::Good, 0);
        assert_eq!(outcome.interval_days, 25); // 10 × 2.5

        // easy 先把 EF 提到 2.65，再乘 1.3：10 × 2.65 × 1.3 = 34.45 → 34
        let easy = schedule_next(state(3, 10), Rating::Easy, 0);
        assert_eq!(easy.ease, 2.65);
        assert_eq!(easy.interval_days, 34);

        // hard 用固定倍率 1.2（不看 EF）：10 × 1.2 = 12，同时 EF 降到 2.35
        let hard = schedule_next(state(3, 10), Rating::Hard, 0);
        assert_eq!(hard.interval_days, 12);
        assert_eq!(hard.ease, 2.35);
    }

    #[test]
    fn interval_and_ease_are_bounded() {
        let outcome = schedule_next(state(5, 170), Rating::Easy, 0);
        assert_eq!(outcome.interval_days, MAX_INTERVAL_DAYS);

        let maxed = SchedulerState { ease: 2.8, ..state(2, 5) };
        assert_eq!(schedule_next(maxed, Rating::Easy, 0).ease, MAX_EASE);

        let floored = SchedulerState { ease: 1.3, ..state(2, 5) };
        assert_eq!(schedule_next(floored, Rating::Again, 0).ease, MIN_EASE);
    }
}
