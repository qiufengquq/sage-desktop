import type { AnalyticsData, AnalyticsTotals, ReviewRecord } from "@/types";
import { average, percentage, sum } from "@/lib/utils";
import {
  dailyStats,
  forgettingCurve,
  masteryTrends,
  reviewQueue,
  streak,
  timeDistribution,
  weeklyStats,
} from "./learning";

function totalsOf(daily: typeof dailyStats): AnalyticsTotals {
  const active = daily.filter((d) => d.minutes > 0);
  const items = sum(daily.map((d) => d.items));
  const correct = sum(daily.map((d) => d.correct));
  const best = [...daily].sort((a, b) => b.minutes - a.minutes)[0] ?? null;
  return {
    minutes: sum(daily.map((d) => d.minutes)),
    items,
    accuracy: items === 0 ? 0 : Math.round((correct / items) * 100),
    activeDays: active.length,
    avgMinutes: Math.round(average(active.map((d) => d.minutes))),
    bestDay: best ? { date: best.date, minutes: best.minutes } : null,
  };
}

function toMastered(records: ReviewRecord[]): ReviewRecord[] {
  return records.filter((r) => r.state === "mastered");
}

export function analyticsFor(range: 7 | 14 | 30): AnalyticsData {
  const daily = dailyStats.slice(-range);
  const mastered = toMastered(reviewQueue);
  return {
    range,
    daily,
    weekly: weeklyStats,
    trends: masteryTrends,
    forgettingCurve,
    timeDistribution: timeDistribution.map((bucket) => ({
      ...bucket,
      // 范围变化时按比例缩放，保证图形与所选区间一致
      minutes: Math.round((bucket.minutes * range) / 30),
    })),
    reviewQueue,
    mastered,
    totals: totalsOf(daily),
    streak,
  };
}

/** 掌握率：用真实统计而非装饰性数字 */
export function masteredRate(records: ReviewRecord[]) {
  return percentage(records.filter((r) => r.state === "mastered").length, records.length);
}
