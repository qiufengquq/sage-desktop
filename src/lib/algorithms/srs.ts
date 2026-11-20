import type { ReviewRating } from "@/types";

/**
 * 复习调度算法（SM-2 变体）。
 *
 * 为什么不用 FSRS：
 * - FSRS 需要为每个条目维护 stability / difficulty 两个隐变量，并依赖一套训练好的权重矩阵；
 *   参数不可解释，用户问"为什么 3 天后又出现"时无法给出简单答案。
 * - 本项目的判分粒度是"部分得分"，FSRS 的评分假设（1–4 离散）需要额外映射，误差来源变多。
 * - 未来要迁到 Rust，SM-2 变体可以用十几个纯函数表达，两端对照实现成本低。
 *
 * 为什么不用第三方库：
 * - 需求只覆盖"初次学习 / 失败重学 / 连续成功 / 间隔增长 / 遗忘降级"，
 *   自实现约 100 行，零依赖、可单测、可逐行解释，收益大于引入依赖。
 *
 * 调度规则（rating 为用户自评）：
 *   again（忘记）：进入重学阶段，10 分钟后再次出现，EF −0.20，lapse +1
 *   hard （模糊）：间隔 ×1.2（最少 1 天），EF −0.15
 *   good （认识）：0→1 天，1→3 天，之后 ×EF
 *   easy （很熟）：0→2 天，1→4 天，之后 ×EF×1.3
 *   EF 限制在 [1.3, 2.8]，间隔上限 180 天。
 */

export const SRS_PARAMS = {
  minEase: 1.3,
  maxEase: 2.8,
  initialEase: 2.5,
  maxIntervalDays: 180,
  /** 重学阶段的下一次出现时间（分钟） */
  relearnMinutes: 10,
  easeDelta: { again: -0.2, hard: -0.15, good: 0, easy: 0.15 },
  firstInterval: { again: 0, hard: 1, good: 1, easy: 2 },
  secondInterval: { again: 0, hard: 2, good: 3, easy: 4 },
} as const;

export interface SchedulerState {
  ease: number;
  intervalDays: number;
  /** 连续成功次数，失败归零 */
  repetition: number;
  lapseCount: number;
}

export interface ScheduleOutcome extends SchedulerState {
  dueAt: string;
  /** 本次调度的人类可读原因，用于复习总结页 */
  explanation: string;
}

const DAY_MS = 24 * 60 * 60 * 1000;
const MINUTE_MS = 60 * 1000;

export function initialSchedulerState(): SchedulerState {
  return {
    ease: SRS_PARAMS.initialEase,
    intervalDays: 0,
    repetition: 0,
    lapseCount: 0,
  };
}

/**
 * 计算下一次复习时间。
 * @param state 当前调度状态
 * @param rating 用户自评
 * @param now 当前时间（显式传入，保证可测试）
 */
export function scheduleNext(
  state: SchedulerState,
  rating: ReviewRating,
  now: Date,
): ScheduleOutcome {
  const ease = clampEase(state.ease + SRS_PARAMS.easeDelta[rating]);
  const repetition = rating === "again" ? 0 : state.repetition + 1;
  const lapseCount = state.lapseCount + (rating === "again" ? 1 : 0);

  let intervalDays: number;
  let explanation: string;

  if (rating === "again") {
    intervalDays = 0;
    explanation = `回到重学阶段，${SRS_PARAMS.relearnMinutes} 分钟后再次出现`;
  } else if (state.repetition === 0) {
    intervalDays = SRS_PARAMS.firstInterval[rating];
    explanation = `首次记住，${intervalDays} 天后复习`;
  } else if (state.repetition === 1) {
    intervalDays = SRS_PARAMS.secondInterval[rating];
    explanation = `第二次成功，间隔延长到 ${intervalDays} 天`;
  } else {
    const multiplier = rating === "easy" ? ease * 1.3 : rating === "hard" ? 1.2 : ease;
    intervalDays = Math.max(1, Math.round(state.intervalDays * multiplier));
    explanation = `按难度因子 ${ease.toFixed(2)} 推算，间隔延长到 ${intervalDays} 天`;
  }

  intervalDays = Math.min(intervalDays, SRS_PARAMS.maxIntervalDays);

  const dueMs =
    intervalDays === 0
      ? now.getTime() + SRS_PARAMS.relearnMinutes * MINUTE_MS
      : now.getTime() + intervalDays * DAY_MS;

  return {
    ease,
    intervalDays,
    repetition,
    lapseCount,
    dueAt: new Date(dueMs).toISOString(),
    explanation,
  };
}

export function isDue(dueAt: string | null, now: Date): boolean {
  if (!dueAt) return false;
  const time = new Date(dueAt).getTime();
  return Number.isFinite(time) && time <= now.getTime();
}

/** 逾期天数：0 表示今天到期，正数表示已逾期 */
export function overdueDaysOf(dueAt: string, now: Date): number {
  const due = new Date(dueAt);
  if (!Number.isFinite(due.getTime())) return 0;
  const dueDay = startOfLocalDay(due).getTime();
  const today = startOfLocalDay(now).getTime();
  return Math.round((today - dueDay) / DAY_MS);
}

function startOfLocalDay(date: Date) {
  const d = new Date(date);
  d.setHours(0, 0, 0, 0);
  return d;
}

function clampEase(ease: number) {
  return Math.min(SRS_PARAMS.maxEase, Math.max(SRS_PARAMS.minEase, ease));
}
