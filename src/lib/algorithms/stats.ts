import type {
  AnalyticsTotals,
  DailyStat,
  EntityState,
  ForgettingCurvePoint,
  MasteryBreakdown,
  MasteryEvent,
  MasteryTrend,
  ReviewLog,
  StreakInfo,
  TimeBucket,
  TrainingAttempt,
  WeeklyStat,
} from "@/types";
import {
  addDaysIn,
  hourIn,
  isoDateIn,
  lastNDaysIn,
  localOffsetMinutes,
  startOfDayIn,
} from "@/lib/date";
import { average, clamp, sum } from "@/lib/utils";

/**
 * 学习统计聚合（纯函数）。
 *
 * 所有指标都从两类原始事实计算：
 * - 训练/测评记录（TrainingAttempt / AssessmentAttempt）
 * - 复习记录（ReviewLog）与掌握度事件（MasteryEvent）
 *
 * 不存在"为了让图表好看"而生成的曲线。唯一的建模假设见 REVIEW_MINUTES_PER_CARD。
 */

/** 一次复习动作按 0.5 分钟计入学习时长（没有逐卡计时，用固定值表述为估算） */
export const REVIEW_MINUTES_PER_CARD = 0.5;

const HOUR_BUCKETS = [
  { label: "06–09", from: 6, to: 9 },
  { label: "09–12", from: 9, to: 12 },
  { label: "12–15", from: 12, to: 15 },
  { label: "15–18", from: 15, to: 18 },
  { label: "18–21", from: 18, to: 21 },
  { label: "21–24", from: 21, to: 24 },
] as const;

export interface DailyStatsInput {
  attempts: TrainingAttempt[];
  reviewLogs: ReviewLog[];
  days: number;
  now: Date;
  /**
   * 用户时区偏移（分钟，东正西负）。归日必须按用户时区，而不是运行环境时区，
   * 否则跨时区/跨零点会把同一次学习算到不同的一天。缺省回退到本地偏移。
   */
  offsetMinutes?: number;
}

/** 生成最近 N 天的每日事实统计（无记录的日期补 0，不插值） */
export function buildDailyStats(input: DailyStatsInput): DailyStat[] {
  const offset = input.offsetMinutes ?? localOffsetMinutes(input.now);
  const dates = lastNDaysIn(input.days, input.now, offset);
  const index = new Map<string, DailyStat>(
    dates.map((date) => [
      date,
      {
        date,
        minutes: 0,
        items: 0,
        correct: 0,
        xp: 0,
        vocabularyReviewed: 0,
        grammarReviewed: 0,
      },
    ]),
  );

  for (const attempt of input.attempts) {
    const bucket = index.get(isoDateIn(attempt.finishedAt, offset));
    if (!bucket) continue;
    bucket.minutes += attempt.minutes;
    bucket.items += attempt.answers.length;
    bucket.correct += attempt.answers.filter((a) => a.correct).length;
    bucket.xp += attempt.xp;
    if (attempt.track === "vocabulary") bucket.vocabularyReviewed += attempt.answers.length;
    if (attempt.track === "grammar") bucket.grammarReviewed += attempt.answers.length;
  }

  for (const log of input.reviewLogs) {
    const bucket = index.get(isoDateIn(log.at, offset));
    if (!bucket) continue;
    bucket.minutes += REVIEW_MINUTES_PER_CARD;
    bucket.xp += log.xp;
    if (log.entityId.startsWith("w_")) bucket.vocabularyReviewed += 1;
    if (log.entityId.startsWith("g_")) bucket.grammarReviewed += 1;
  }

  return dates.map((date) => {
    const bucket = index.get(date) as DailyStat;
    return { ...bucket, minutes: Math.round(bucket.minutes) };
  });
}

/** 连续学习天数：今天没学但昨天学了，仍算连续（当天还没开始） */
export function computeStreak(daily: DailyStat[]): StreakInfo {
  const active = (day: DailyStat | undefined) => Boolean(day && day.minutes > 0);

  let current = 0;
  let cursor = daily.length - 1;
  if (cursor >= 0 && !active(daily[cursor])) cursor -= 1;
  while (cursor >= 0 && active(daily[cursor])) {
    current += 1;
    cursor -= 1;
  }

  let longest = 0;
  let run = 0;
  for (const day of daily) {
    if (day.minutes > 0) {
      run += 1;
      longest = Math.max(longest, run);
    } else {
      run = 0;
    }
  }

  return {
    current,
    longest: Math.max(longest, current),
    recent: daily.slice(-14).map((day) => ({
      date: day.date,
      active: day.minutes > 0,
      minutes: day.minutes,
    })),
  };
}

export function totalsFrom(daily: DailyStat[]): AnalyticsTotals {
  const items = sum(daily.map((d) => d.items));
  const correct = sum(daily.map((d) => d.correct));
  const activeDays = daily.filter((d) => d.minutes > 0).length;
  const best = [...daily].sort((a, b) => b.minutes - a.minutes)[0] ?? null;

  return {
    minutes: sum(daily.map((d) => d.minutes)),
    items,
    accuracy: items === 0 ? 0 : Math.round((correct / items) * 100),
    activeDays,
    avgMinutes: Math.round(average(daily.filter((d) => d.minutes > 0).map((d) => d.minutes))),
    bestDay: best && best.minutes > 0 ? { date: best.date, minutes: best.minutes } : null,
  };
}

/** 按自然周（周一起）聚合最近 5 周 */
export function weeklyFrom(
  daily: DailyStat[],
  now: Date,
  offsetMinutes = localOffsetMinutes(now),
): WeeklyStat[] {
  const weeks: WeeklyStat[] = [];
  const today = startOfDayIn(now, offsetMinutes);

  for (let i = 0; i < 5; i += 1) {
    const end = addDaysIn(today, -7 * i, offsetMinutes);
    const start = addDaysIn(end, -6, offsetMinutes);
    const startIso = isoDateIn(start, offsetMinutes);
    const endIso = isoDateIn(end, offsetMinutes);
    const slice = daily.filter((d) => d.date >= startIso && d.date <= endIso);
    const items = sum(slice.map((d) => d.items));
    const correct = sum(slice.map((d) => d.correct));

    weeks.push({
      label: `第 ${isoWeekNumber(start)} 周`,
      weekStart: startIso,
      minutes: sum(slice.map((d) => d.minutes)),
      items,
      accuracy: items === 0 ? 0 : Math.round((correct / items) * 100),
      activeDays: slice.filter((d) => d.minutes > 0).length,
    });
  }

  return weeks.reverse();
}

/** 学习时间分布：按开始作答的小时归入 3 小时一段（小时取用户时区） */
export function timeBuckets(
  attempts: TrainingAttempt[],
  reviewLogs: ReviewLog[],
  offsetMinutes?: number,
): TimeBucket[] {
  const offset = offsetMinutes ?? localOffsetMinutes();
  const buckets = HOUR_BUCKETS.map((bucket) => ({
    label: bucket.label,
    minutes: 0,
    items: 0,
    correct: 0,
  }));

  const push = (at: string, minutes: number, items: number, correct: number) => {
    const hour = hourIn(at, offset);
    const index = HOUR_BUCKETS.findIndex((b) => hour >= b.from && hour < b.to);
    if (index < 0) return;
    const bucket = buckets[index];
    if (!bucket) return;
    bucket.minutes += minutes;
    bucket.items += items;
    bucket.correct += correct;
  };

  for (const attempt of attempts) {
    push(
      attempt.startedAt,
      attempt.minutes,
      attempt.answers.length,
      attempt.answers.filter((a) => a.correct).length,
    );
  }
  for (const log of reviewLogs) {
    push(log.at, REVIEW_MINUTES_PER_CARD, 1, log.rating === "again" ? 0 : 1);
  }

  return buckets.map((bucket) => ({
    label: bucket.label,
    minutes: Math.round(bucket.minutes),
    accuracy: bucket.items === 0 ? 0 : Math.round((bucket.correct / bucket.items) * 100),
  }));
}

const TREND_KIND_MAP: Record<MasteryTrend["kind"], EntityState["kind"][]> = {
  vocabulary: ["word", "phrase"],
  grammar: ["grammar"],
  speaking: [],
};

/**
 * 掌握趋势。
 * - 词汇/语法：用 MasteryEvent 精确回放（当前值向前反推每日增量）。
 * - 口语：没有实体掌握度，改用每日口语训练正确率，这一点在 UI 上会单独说明。
 */
export function masteryTrendOf(input: {
  kind: MasteryTrend["kind"];
  states: Record<string, EntityState>;
  events: MasteryEvent[];
  attempts: TrainingAttempt[];
  days: number;
  now: Date;
  /** 用户时区偏移（分钟），归日口径必须与其它统计一致 */
  offsetMinutes?: number;
}): MasteryTrend {
  const offset = input.offsetMinutes ?? localOffsetMinutes(input.now);
  const dates = lastNDaysIn(input.days, input.now, offset);
  const labels: Record<MasteryTrend["kind"], string> = {
    vocabulary: "词汇",
    grammar: "语法",
    speaking: "口语",
  };

  let series: number[];

  if (input.kind === "speaking") {
    const byDay = new Map<string, { items: number; correct: number }>();
    for (const attempt of input.attempts) {
      if (attempt.track !== "speaking") continue;
      const key = isoDateIn(attempt.finishedAt, offset);
      const bucket = byDay.get(key) ?? { items: 0, correct: 0 };
      bucket.items += attempt.answers.length;
      bucket.correct += attempt.answers.filter((a) => a.correct).length;
      byDay.set(key, bucket);
    }
    let carry = 0;
    series = dates.map((date) => {
      const bucket = byDay.get(date);
      if (bucket && bucket.items > 0) carry = Math.round((bucket.correct / bucket.items) * 100);
      return carry;
    });
  } else {
    const kinds = TREND_KIND_MAP[input.kind];
    const relevant = Object.values(input.states).filter((s) => kinds.includes(s.kind));
    const current =
      relevant.length === 0 ? 0 : Math.round(average(relevant.map((s) => s.mastery)));

    const deltaByDay = new Map<string, number>();
    for (const event of input.events) {
      if (!kinds.includes(event.kind)) continue;
      const key = isoDateIn(event.at, offset);
      deltaByDay.set(key, (deltaByDay.get(key) ?? 0) + (event.after - event.before));
    }

    series = new Array<number>(dates.length).fill(current);
    for (let i = dates.length - 2; i >= 0; i -= 1) {
      const nextDate = dates[i + 1] as string;
      const delta = deltaByDay.get(nextDate) ?? 0;
      series[i] = Math.round(clamp((series[i + 1] as number) - delta, 0, 100));
    }
  }

  const current = series[series.length - 1] ?? 0;
  const first = series[0] ?? 0;

  return {
    kind: input.kind,
    label: labels[input.kind],
    current,
    delta: current - first,
    series: dates.map((date, i) => ({ date, value: series[i] ?? 0 })),
  };
}

/**
 * 遗忘曲线。
 * 理论留存：指数衰减，稳定度 3 天。
 * 你的留存：稳定度由平均记忆强度推算，是模型估计值，不是逐条实测。
 */
export function forgettingCurveOf(states: EntityState[]): ForgettingCurvePoint[] {
  const hours = [1, 6, 24, 72, 168, 336, 720];
  const avgStrength = states.length === 0 ? 0 : average(states.map((s) => s.strength));
  const actualStability = 1 + avgStrength * 13;

  return hours.map((h) => ({
    hours: h,
    expected: Math.round(100 * Math.exp(-h / (24 * 3))),
    actual: Math.round(100 * Math.exp(-h / (24 * actualStability))),
  }));
}

export function masteryBreakdown(states: EntityState[]): MasteryBreakdown {
  return {
    mastered: states.filter((s) => s.state === "mastered").length,
    reviewing: states.filter((s) => s.state === "reviewing").length,
    learning: states.filter((s) => s.state === "learning").length,
    new: states.filter((s) => s.state === "new").length,
    lapsed: states.filter((s) => s.state === "lapsed").length,
  };
}

export function averageMastery(states: EntityState[]): number {
  return Math.round(average(states.map((s) => s.mastery)));
}

function isoWeekNumber(date: Date): number {
  const d = new Date(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()));
  const day = d.getUTCDay() || 7;
  d.setUTCDate(d.getUTCDate() + 4 - day);
  const yearStart = new Date(Date.UTC(d.getUTCFullYear(), 0, 1));
  return Math.ceil(((d.getTime() - yearStart.getTime()) / 86400000 + 1) / 7);
}
