//! 经验与等级（对照组：`src/lib/algorithms/xp.ts`）。
//!
//! XP 不做独立累计表：总数由 `training_sessions.xp + review_logs.xp` 求和得到，
//! 避免"计数器与实际记录不一致"。

use super::srs::Rating;

pub const REVIEW_XP: i64 = 4;
pub const XP_BASE: f64 = 500.0;
pub const XP_GROWTH: f64 = 1.12;

#[derive(Debug, Clone, Copy)]
pub struct XpProgress {
    pub total: i64,
    pub level: i64,
    pub current: i64,
    pub required: i64,
    pub progress: f64,
}

pub fn review_rating_bonus(rating: Rating) -> i64 {
    match rating {
        Rating::Again => 0,
        Rating::Hard => 1,
        Rating::Good => 2,
        Rating::Easy => 3,
    }
}

pub fn xp_for_review(rating: Rating) -> i64 {
    REVIEW_XP + review_rating_bonus(rating)
}

pub fn level_from_xp(total: i64) -> XpProgress {
    let safe_total = total.max(0);
    let mut level: i64 = 1;
    let mut remaining = safe_total;
    let mut required = XP_BASE.round() as i64;

    while remaining >= required {
        remaining -= required;
        level += 1;
        required = (XP_BASE * XP_GROWTH.powi((level - 1) as i32)).round() as i64;
    }

    let progress = if required == 0 {
        0.0
    } else {
        ((remaining as f64 / required as f64) * 100.0).round() / 100.0
    };

    XpProgress {
        total: safe_total,
        level,
        current: remaining,
        required,
        progress,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_xp_includes_rating_bonus() {
        assert_eq!(xp_for_review(Rating::Again), 4);
        assert_eq!(xp_for_review(Rating::Good), 6);
        assert_eq!(xp_for_review(Rating::Easy), 7);
    }

    #[test]
    fn level_curve_starts_at_500_and_grows() {
        assert_eq!(level_from_xp(0).level, 1);
        assert_eq!(level_from_xp(499).level, 1);
        assert_eq!(level_from_xp(500).level, 2);
        assert_eq!(level_from_xp(500).current, 0);

        let level2 = level_from_xp(500).required;
        assert_eq!(level2, 560, "500 × 1.12");
    }
}
