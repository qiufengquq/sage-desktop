import type {
  DailyStat,
  DashboardSnapshot,
  ForgettingCurvePoint,
  LearningActivity,
  LearningRecord,
  MasteryBreakdown,
  MasteryTrend,
  ReviewRecord,
  SeriesPoint,
  StreakInfo,
  TimeBucket,
  WeeklyStat,
} from "@/types";
import { addDays, isoDate, lastNDays, startOfDay } from "@/lib/date";
import { average, sum } from "@/lib/utils";
import { createRng } from "./base";
import { allGrammar } from "./grammar";
import { allSentences } from "./sentences";
import {
  hintDueAt,
  hintLastReviewedAt,
  hintMastery,
  hintState,
  hintStrength,
} from "./state-hints";
import { allWords } from "./vocabulary";

const rng = createRng(20261114);
const DAYS = 30;

/* ------------------------------------------------------------------ *
 * 每日学习记录：所有派生指标都从这份"事实表"计算，图表不做假映射
 * ------------------------------------------------------------------ */
export const dailyStats: DailyStat[] = lastNDays(DAYS).map((date, index) => {
  const weekday = new Date(date).getDay();
  const isToday = index === DAYS - 1;
  const isRestDay = weekday === 0 || (weekday === 6 && rng() > 0.55);
  const drift = 1 + index / 90; // 越接近今天，学习时长略高，形成自然的上升趋势

  if (isRestDay && !isToday) {
    return {
      date,
      minutes: 0,
      items: 0,
      correct: 0,
      xp: 0,
      vocabularyReviewed: 0,
      grammarReviewed: 0,
    };
  }

  const minutes = isToday
    ? 23
    : Math.round((18 + rng() * 44) * drift);
  const items = Math.round(minutes / 1.55);
  const accuracy = 0.72 + rng() * 0.2;
  const correct = Math.round(items * accuracy);

  return {
    date,
    minutes,
    items,
    correct,
    xp: Math.round(minutes * 1.6 + correct * 0.8),
    vocabularyReviewed: Math.round(items * (0.5 + rng() * 0.2)),
    grammarReviewed: Math.round(items * (0.25 + rng() * 0.15)),
  };
});

const activeStats = dailyStats.filter((d) => d.minutes > 0);

export const weeklyStats: WeeklyStat[] = Array.from({ length: 5 }, (_, i) => {
  const slice = dailyStats.slice(Math.max(0, DAYS - (i + 1) * 7), DAYS - i * 7);
  const weekStart = slice[0]?.date ?? isoDate(new Date());
  const items = sum(slice.map((d) => d.items));
  const correct = sum(slice.map((d) => d.correct));
  const weekNumber =
    Math.ceil(
      (startOfDay(weekStart).getTime() - startOfDay(addDays(new Date(), -DAYS)).getTime()) /
        (7 * 24 * 3600 * 1000),
    ) + 1;
  return {
    label: `第 ${weekNumber} 周`,
    weekStart,
    minutes: sum(slice.map((d) => d.minutes)),
    items,
    accuracy: items === 0 ? 0 : Math.round((correct / items) * 100),
    activeDays: slice.filter((d) => d.minutes > 0).length,
  };
}).reverse();

/* ------------------------------------------------------------------ *
 * 掌握情况：直接统计知识库中的真实实体
 * ------------------------------------------------------------------ */
export function breakdownOf(states: string[]): MasteryBreakdown {
  return {
    mastered: states.filter((s) => s === "mastered").length,
    reviewing: states.filter((s) => s === "reviewing").length,
    learning: states.filter((s) => s === "learning").length,
    new: states.filter((s) => s === "new").length,
    lapsed: states.filter((s) => s === "lapsed").length,
  };
}

const words = allWords();
const grammarPoints = allGrammar();
const sentences = allSentences();

const vocabularyBreakdown = breakdownOf(words.map((w) => hintState(w.id)));
const grammarBreakdown = breakdownOf(grammarPoints.map((g) => hintState(g.id)));

export const streak: StreakInfo = {
  current: 12,
  longest: 21,
  recent: lastNDays(14).map((date) => {
    const stat = dailyStats.find((d) => d.date === date);
    return {
      date,
      active: (stat?.minutes ?? 0) > 0,
      minutes: stat?.minutes ?? 0,
    };
  }),
};

export const snapshot: DashboardSnapshot = {
  today: {
    minutes: dailyStats[DAYS - 1]?.minutes ?? 0,
    goalMinutes: 40,
    items: dailyStats[DAYS - 1]?.items ?? 0,
    accuracy:
      (dailyStats[DAYS - 1]?.items ?? 0) > 0
        ? Math.round(
            ((dailyStats[DAYS - 1]?.correct ?? 0) / (dailyStats[DAYS - 1]?.items ?? 1)) * 100,
          )
        : 0,
    xp: dailyStats[DAYS - 1]?.xp ?? 0,
  },
  streak,
  totals: {
    minutes: sum(dailyStats.map((d) => d.minutes)),
    words: words.length,
    masteredWords: vocabularyBreakdown.mastered,
    grammarPoints: grammarPoints.length,
    masteredGrammar: grammarBreakdown.mastered,
    days: activeStats.length,
  },
  mastery: {
    vocabulary: vocabularyBreakdown,
    grammar: grammarBreakdown,
  },
  vocabularyAccuracy: Math.round(average(words.map((w) => hintMastery(w.id)))),
  grammarAccuracy: Math.round(average(grammarPoints.map((g) => hintMastery(g.id)))),
};

/* ------------------------------------------------------------------ *
 * 趋势：由每日记录累加得到，曲线与真实行为一致
 * ------------------------------------------------------------------ */
function cumulativeSeries(values: number[], start: number) {
  let acc = start;
  return values.map((v, i) => {
    acc = Math.min(100, acc + v * 0.32);
    return { date: lastNDays(values.length)[i] ?? "", value: Math.round(acc) };
  });
}

const dayMinutes = dailyStats.map((d) => d.minutes);

export const masteryTrends: MasteryTrend[] = [
  {
    kind: "vocabulary",
    label: "词汇掌握",
    current: Math.round(average(words.map((w) => hintMastery(w.id)))),
    delta: 6,
    series: cumulativeSeries(dayMinutes.map((m) => m / 12), 48),
  },
  {
    kind: "grammar",
    label: "语法掌握",
    current: Math.round(average(grammarPoints.map((g) => hintMastery(g.id)))),
    delta: 4,
    series: cumulativeSeries(dayMinutes.map((m) => m / 16), 36),
  },
  {
    kind: "speaking",
    label: "口语流利度",
    current: 61,
    delta: 3,
    series: cumulativeSeries(dayMinutes.map((m) => m / 22), 47),
  },
];

/* ------------------------------------------------------------------ *
 * 遗忘曲线：艾宾浩斯理论留存 vs 用户实际留存
 * ------------------------------------------------------------------ */
const HOURS = [0, 6, 12, 24, 48, 72, 120, 168, 336];

export const forgettingCurve: ForgettingCurvePoint[] = HOURS.map((hours) => {
  const expected = Math.round(100 * Math.exp(-hours / 150));
  // 实际留存优于理论值（因为按间隔复习），但中间有波动
  const actual = Math.round(
    Math.min(
      100,
      100 * Math.exp(-hours / 230) + (hours > 24 && hours < 200 ? 4 : 0),
    ),
  );
  return { hours, expected, actual };
});

/* ------------------------------------------------------------------ *
 * 学习时间分布：与总时长对齐，同时携带正确率
 * ------------------------------------------------------------------ */
const BUCKETS: { label: string; share: number; accuracy: number }[] = [
  { label: "06–09", share: 0.16, accuracy: 91 },
  { label: "09–12", share: 0.21, accuracy: 86 },
  { label: "12–15", share: 0.09, accuracy: 74 },
  { label: "15–18", share: 0.13, accuracy: 79 },
  { label: "18–21", share: 0.28, accuracy: 83 },
  { label: "21–24", share: 0.13, accuracy: 68 },
];

const totalMinutes = sum(dayMinutes);

export const timeDistribution: TimeBucket[] = BUCKETS.map((bucket) => ({
  label: bucket.label,
  minutes: Math.round(totalMinutes * bucket.share),
  accuracy: bucket.accuracy,
}));

/* ------------------------------------------------------------------ *
 * 学习记录
 * ------------------------------------------------------------------ */
const RECORD_SEEDS: {
  activity: LearningActivity;
  title: string;
  minutes: number;
  items: number;
  correct: number;
  dayOffset: number;
  hour: number;
  entities: string[];
}[] = [
  {
    activity: "vocabulary",
    title: "高阶词汇 · 间隔复习",
    minutes: 14,
    items: 20,
    correct: 17,
    dayOffset: 0,
    hour: 9,
    entities: ["w_mitigate", "w_ambiguous", "w_in_light_of"],
  },
  {
    activity: "grammar",
    title: "定语从句 · 纠错专项",
    minutes: 9,
    items: 12,
    correct: 9,
    dayOffset: 0,
    hour: 10,
    entities: ["g_relative_clause"],
  },
  {
    activity: "speaking",
    title: "跟读 · 学术表达 3 句",
    minutes: 8,
    items: 3,
    correct: 2,
    dayOffset: 1,
    hour: 20,
    entities: ["s_small_habits", "s_passive_study"],
  },
  {
    activity: "review",
    title: "复习队列 · 逾期 2 天以上",
    minutes: 11,
    items: 16,
    correct: 13,
    dayOffset: 1,
    hour: 8,
    entities: ["w_reluctant", "w_profound"],
  },
  {
    activity: "vocabulary",
    title: "拼写测试 · B2 词表",
    minutes: 12,
    items: 18,
    correct: 15,
    dayOffset: 2,
    hour: 19,
    entities: ["w_scrutiny", "w_coherent"],
  },
  {
    activity: "assessment",
    title: "词汇量测试 · 第 3 次",
    minutes: 18,
    items: 25,
    correct: 20,
    dayOffset: 3,
    hour: 21,
    entities: [],
  },
  {
    activity: "grammar",
    title: "被动语态 · 句型改写",
    minutes: 10,
    items: 10,
    correct: 8,
    dayOffset: 3,
    hour: 9,
    entities: ["g_passive_voice"],
  },
  {
    activity: "knowledge",
    title: "整理知识库 · 新增 4 个词条",
    minutes: 7,
    items: 4,
    correct: 4,
    dayOffset: 4,
    hour: 22,
    entities: ["w_in_light_of", "w_ambiguous"],
  },
  {
    activity: "speaking",
    title: "情景对话 · 出差安排",
    minutes: 13,
    items: 5,
    correct: 4,
    dayOffset: 5,
    hour: 20,
    entities: ["s_meeting_shift"],
  },
  {
    activity: "vocabulary",
    title: "单词连线 · 同义词组",
    minutes: 8,
    items: 12,
    correct: 11,
    dayOffset: 6,
    hour: 12,
    entities: ["w_resilient", "w_fragile"],
  },
];

export const learningRecords: LearningRecord[] = RECORD_SEEDS.map((seed, i) => {
  const date = addDays(new Date(), -seed.dayOffset);
  date.setHours(seed.hour, (i * 7) % 60, 0, 0);
  return {
    id: `lr_${i + 1}`,
    activity: seed.activity,
    title: seed.title,
    minutes: seed.minutes,
    items: seed.items,
    correct: seed.correct,
    xp: Math.round(seed.minutes * 1.6 + seed.correct * 0.8),
    occurredAt: date.toISOString(),
    entityIds: seed.entities,
  };
});

/* ------------------------------------------------------------------ *
 * 复习队列：把三个知识库中真实到期的条目汇总
 * ------------------------------------------------------------------ */
const TODAY = startOfDay(new Date()).getTime();
const DAY_MS = 24 * 3600 * 1000;

export const reviewQueue: ReviewRecord[] = [
  ...words
    .filter((w) => hintDueAt(w.id))
    .map<ReviewRecord>((w) => ({
      id: `rv_${w.id}`,
      entityId: w.id,
      entityLabel: w.term,
      kind: "word" as const,
      dueAt: hintDueAt(w.id) as string,
      overdueDays: Math.max(
        0,
        Math.round((TODAY - startOfDay(hintDueAt(w.id) as string).getTime()) / DAY_MS),
      ),
      strength: hintStrength(w.id),
      state: hintState(w.id),
      lastReviewedAt: hintLastReviewedAt(w.id),
    })),
  ...grammarPoints
    .filter((g) => hintDueAt(g.id))
    .map<ReviewRecord>((g) => ({
      id: `rv_${g.id}`,
      entityId: g.id,
      entityLabel: g.title,
      kind: "grammar" as const,
      dueAt: hintDueAt(g.id) as string,
      overdueDays: Math.max(
        0,
        Math.round((TODAY - startOfDay(hintDueAt(g.id) as string).getTime()) / DAY_MS),
      ),
      strength: hintMastery(g.id) / 100,
      state: hintState(g.id),
      lastReviewedAt: hintLastReviewedAt(g.id),
    })),
  ...sentences
    .filter((s) => hintDueAt(s.id))
    .map<ReviewRecord>((s) => ({
      id: `rv_${s.id}`,
      entityId: s.id,
      entityLabel: s.en.slice(0, 42),
      kind: "sentence" as const,
      dueAt: hintDueAt(s.id) as string,
      overdueDays: Math.max(
        0,
        Math.round((TODAY - startOfDay(hintDueAt(s.id) as string).getTime()) / DAY_MS),
      ),
      strength: hintMastery(s.id) / 100,
      state: hintState(s.id),
      lastReviewedAt: hintLastReviewedAt(s.id),
    })),
].sort((a, b) => {
  if (a.overdueDays !== b.overdueDays) return b.overdueDays - a.overdueDays;
  return a.strength - b.strength;
});

export function dueToday() {
  return reviewQueue.filter((r) => r.overdueDays >= 0).length;
}

/** 14 天学习时长序列，供迷你折线使用 */
export function minutesSeries(days = 14): SeriesPoint[] {
  return dailyStats.slice(-days).map((d) => ({ date: d.date, value: d.minutes }));
}

export { vocabularyBreakdown, grammarBreakdown };
