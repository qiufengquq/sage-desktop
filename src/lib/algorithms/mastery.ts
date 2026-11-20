import type { Difficulty, MasteryState } from "@/types";
import { clamp } from "@/lib/utils";

/**
 * 掌握度更新算法（纯函数，可测试、可解释、与 UI 解耦）。
 *
 * 设计目标：
 * 1. 答对一次不会直接到 100：使用"目标值 + 学习率"的逐步逼近，而不是覆盖写。
 * 2. 遗忘会先扣分：距上次复习超过 7 天后，掌握度按天衰减，再叠加本次表现。
 * 3. 难度参与计算：难题答对信息量更大，学习率更高；简单题答对提升有限。
 * 4. 每次更新都给出因子明细，便于 UI 解释"为什么涨了 / 掉了"。
 *
 * 公式（单次作答）：
 *   decayed   = 当前掌握度 × 衰减系数
 *   target    = 答对 ? 70 + 30 × 得分率 : 40 × 得分率
 *   learning  = clamp(0.10 + 0.035 × 难度 + 新手加成 - 老手折扣, 0.06, 0.30)
 *   next      = decayed + learning × (target - decayed)
 */

export const MASTERY_PARAMS = {
  /** 判定"答对"的最低得分率 */
  correctThreshold: 0.7,
  /** 判定"明显错误"的上限 */
  failureThreshold: 0.35,
  /** 从第几天开始衰减 */
  decayAfterDays: 7,
  /** 每天衰减的掌握度点数 */
  decayPerDay: 0.6,
  /** 单次衰减上限（占当前掌握度的比例） */
  maxDecayRatio: 0.5,
  /** 记忆强度衰减起始天数 */
  strengthDecayAfterDays: 14,
  /** 记忆强度每周衰减比例 */
  strengthDecayPerWeek: 0.12,
  /** 达到"已掌握"所需的最低掌握度 */
  masteredAt: 80,
  /** 达到"已掌握"所需的最少作答次数 */
  masteredReviews: 3,
  /** 进入"复习中"的最低掌握度 */
  reviewingAt: 40,
  /** 与复习调度保持一致的上限，避免掌握度长期停滞 */
  maxEase: 2.8,
} as const;

export interface MasterySnapshot {
  mastery: number;
  strength: number;
  state: MasteryState;
  reviewCount: number;
  lapseCount: number;
  lastReviewedAt: string | null;
}

export interface MasteryFactor {
  key: "accuracy" | "difficulty" | "recency";
  label: string;
  /** 对掌握度的影响点数，正负号即方向 */
  impact: number;
}

export interface MasteryUpdate {
  previous: number;
  next: number;
  delta: number;
  strength: number;
  state: MasteryState;
  reviewCount: number;
  lapseCount: number;
  /** 本次是否被判定为遗忘（用于 UI 提示与调度降级） */
  lapsed: boolean;
  factors: MasteryFactor[];
}

export interface MasteryInput {
  snapshot: MasterySnapshot;
  /** 判分给出的 0–1 部分得分 */
  partialScore: number;
  difficulty: Difficulty;
  now: Date;
}

const DAY_MS = 24 * 60 * 60 * 1000;

function daysSince(iso: string | null, now: Date): number | null {
  if (!iso) return null;
  const diff = now.getTime() - new Date(iso).getTime();
  if (!Number.isFinite(diff)) return null;
  return Math.max(0, diff / DAY_MS);
}

/** 按时间衰减后的掌握度（不做四舍五入，保留给后续计算） */
export function decayMastery(snapshot: MasterySnapshot, now: Date): number {
  const days = daysSince(snapshot.lastReviewedAt, now);
  if (days === null || days <= MASTERY_PARAMS.decayAfterDays) return snapshot.mastery;
  const overdue = days - MASTERY_PARAMS.decayAfterDays;
  const penalty = Math.min(
    snapshot.mastery * MASTERY_PARAMS.maxDecayRatio,
    overdue * MASTERY_PARAMS.decayPerDay,
  );
  return clamp(snapshot.mastery - penalty, 0, 100);
}

/** 按时间衰减后的记忆强度（0–1） */
export function decayStrength(snapshot: MasterySnapshot, now: Date): number {
  const days = daysSince(snapshot.lastReviewedAt, now);
  if (days === null || days <= MASTERY_PARAMS.strengthDecayAfterDays) return snapshot.strength;
  const weeks = (days - MASTERY_PARAMS.strengthDecayAfterDays) / 7;
  const keep = Math.max(0.2, 1 - weeks * MASTERY_PARAMS.strengthDecayPerWeek);
  return clamp(snapshot.strength * keep, 0, 1);
}

function learningRate(snapshot: MasterySnapshot, difficulty: Difficulty): number {
  const novelty = snapshot.reviewCount < 2 ? 0.06 : snapshot.reviewCount > 8 ? -0.04 : 0;
  return clamp(0.1 + 0.035 * difficulty + novelty, 0.06, 0.3);
}

function nextState(mastery: number, reviewCount: number, lapsed: boolean): MasteryState {
  if (lapsed) return "lapsed";
  if (mastery >= MASTERY_PARAMS.masteredAt && reviewCount >= MASTERY_PARAMS.masteredReviews) {
    return "mastered";
  }
  if (mastery >= MASTERY_PARAMS.reviewingAt) return "reviewing";
  if (mastery > 0) return "learning";
  return "new";
}

/**
 * 单次作答后的掌握度更新。
 * 同一道题的多实体（entityIds 可能不止一个）分别调用本函数。
 */
export function updateMastery(input: MasteryInput): MasteryUpdate {
  const { snapshot, difficulty, now } = input;
  const partial = clamp(input.partialScore, 0, 1);
  const correct = partial >= MASTERY_PARAMS.correctThreshold;
  const decayed = decayMastery(snapshot, now);
  const decayImpact = decayed - snapshot.mastery;

  const baseTarget = correct ? 70 + 30 * partial : 40 * partial;
  const difficultyImpact = correct ? (difficulty - 3) * 2 : 0;
  const target = clamp(baseTarget + difficultyImpact, 0, 100);

  const rate = learningRate(snapshot, difficulty);
  const rawNext = decayed + rate * (target - decayed);
  const next = clamp(rawNext, 0, 100);

  const reviewCount = snapshot.reviewCount + 1;
  const lapsed =
    partial < MASTERY_PARAMS.failureThreshold &&
    (snapshot.state === "mastered" || snapshot.mastery >= 70);
  const lapseCount = snapshot.lapseCount + (partial < MASTERY_PARAMS.failureThreshold ? 1 : 0);

  const decayedStrength = decayStrength(snapshot, now);
  const strength = clamp(
    decayedStrength * 0.7 + partial * 0.3,
    0,
    1,
  );

  // 因子拆解：先算"纯难度贡献"，剩下的归因给作答表现
  const withoutDifficulty = clamp(decayed + rate * (baseTarget - decayed), 0, 100);
  const difficultyContribution = next - withoutDifficulty;

  const factors: MasteryFactor[] = [
    {
      key: "accuracy",
      label: correct ? "作答正确" : partial > 0 ? "部分正确" : "作答错误",
      impact: round1(next - decayed - difficultyContribution),
    },
    {
      key: "difficulty",
      label: `题目难度 ${difficulty}/5`,
      impact: round1(difficultyContribution),
    },
  ];
  if (Math.abs(decayImpact) >= 0.1) {
    factors.push({
      key: "recency",
      label: "距上次复习过久，先有衰减",
      impact: round1(decayImpact),
    });
  }

  return {
    previous: round1(snapshot.mastery),
    next: round1(next),
    delta: round1(next - snapshot.mastery),
    strength: Math.round(strength * 1000) / 1000,
    state: nextState(next, reviewCount, lapsed),
    reviewCount,
    lapseCount,
    lapsed,
    factors,
  };
}

function round1(value: number) {
  return Math.round(value * 10) / 10;
}
