import type { ReviewRating } from "@/types";

/** 每次复习动作的基础经验，强调"复习也算学习" */
export const REVIEW_XP = 4;

/** 复习自评带来的额外经验 */
export const REVIEW_RATING_BONUS: Record<ReviewRating, number> = {
  again: 0,
  hard: 1,
  good: 2,
  easy: 3,
};

/** 升到下一级所需经验：基础 500，逐级递增 12% */
export const XP_BASE = 500;
export const XP_GROWTH = 1.12;

export interface XpProgress {
  total: number;
  level: number;
  /** 当前等级内已获得经验 */
  current: number;
  /** 当前等级所需经验 */
  required: number;
  /** 0–1 */
  progress: number;
}

export function xpForReview(rating: ReviewRating): number {
  return REVIEW_XP + REVIEW_RATING_BONUS[rating];
}

export function levelFromXp(total: number): XpProgress {
  const safeTotal = Math.max(0, Math.round(total));
  let level = 1;
  let remaining = safeTotal;
  let required = XP_BASE;

  while (remaining >= required) {
    remaining -= required;
    level += 1;
    required = Math.round(XP_BASE * Math.pow(XP_GROWTH, level - 1));
  }

  return {
    total: safeTotal,
    level,
    current: remaining,
    required,
    progress: required === 0 ? 0 : Math.round((remaining / required) * 100) / 100,
  };
}
