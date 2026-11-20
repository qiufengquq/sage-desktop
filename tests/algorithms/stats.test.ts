import { describe, expect, it } from "vitest";
import {
  buildDailyStats,
  computeStreak,
  forgettingCurveOf,
  masteryBreakdown,
  masteryTrendOf,
  totalsFrom,
  weeklyFrom,
} from "@/lib/algorithms/stats";
import type { MasteryEvent } from "@/types";
import { NOW, entityState, reviewLog, trainingAttempt } from "../helpers/fixtures";

describe("学习统计聚合", () => {
  it("把训练与复习记录归入正确的自然日", () => {
    const daily = buildDailyStats({
      attempts: [trainingAttempt({ minutes: 12 })],
      reviewLogs: [reviewLog(), reviewLog({ id: "rl_2" })],
      days: 7,
      now: NOW,
    });

    const today = daily[daily.length - 1];
    expect(today?.date).toBe("2026-11-14");
    expect(today?.minutes).toBe(13);
    expect(today?.items).toBe(2);
    expect(today?.correct).toBe(1);
    expect(today?.xp).toBe(30 + 6 + 6);
    expect(today?.vocabularyReviewed).toBe(4);
  });

  it("没有记录的日期补 0，不做插值", () => {
    const daily = buildDailyStats({ attempts: [], reviewLogs: [], days: 5, now: NOW });
    expect(daily).toHaveLength(5);
    expect(daily.every((day) => day.minutes === 0)).toBe(true);
  });

  it("今天还没学但昨天学了，连续天数仍然成立", () => {
    const daily = buildDailyStats({
      attempts: [
        trainingAttempt({ id: "a1", finishedAt: new Date(NOW.getTime() - 86400000).toISOString() }),
        trainingAttempt({ id: "a2", finishedAt: new Date(NOW.getTime() - 2 * 86400000).toISOString() }),
      ],
      reviewLogs: [],
      days: 7,
      now: NOW,
    });

    const streak = computeStreak(daily);
    expect(streak.current).toBe(2);
    expect(streak.recent).toHaveLength(7);
  });

  it("统计总量与最佳单日", () => {
    const daily = buildDailyStats({
      attempts: [
        trainingAttempt({ id: "a1", minutes: 20 }),
        trainingAttempt({ id: "a2", minutes: 5, finishedAt: new Date(NOW.getTime() - 86400000).toISOString() }),
      ],
      reviewLogs: [],
      days: 7,
      now: NOW,
    });

    const totals = totalsFrom(daily);
    expect(totals.minutes).toBe(25);
    expect(totals.activeDays).toBe(2);
    expect(totals.bestDay?.minutes).toBe(20);
    expect(totals.items).toBe(4);
    expect(totals.accuracy).toBe(50);
  });

  it("按自然周聚合，最多 5 周", () => {
    const daily = buildDailyStats({
      attempts: [trainingAttempt({ minutes: 30 })],
      reviewLogs: [],
      days: 30,
      now: NOW,
    });
    const weeks = weeklyFrom(daily, NOW);
    expect(weeks).toHaveLength(5);
    expect(weeks[weeks.length - 1]?.minutes).toBe(30);
  });

  it("掌握趋势可从掌握度事件精确回放", () => {
    const states = {
      w_a: entityState({ entityId: "w_a", mastery: 60 }),
      w_b: entityState({ entityId: "w_b", mastery: 60 }),
    };
    const events: MasteryEvent[] = [
      {
        id: "me_1",
        entityId: "w_a",
        kind: "word",
        before: 45,
        after: 55,
        source: "training",
        refId: "ta_1",
        at: new Date(NOW.getTime() - 86400000).toISOString(),
      },
      {
        id: "me_2",
        entityId: "w_b",
        kind: "word",
        before: 50,
        after: 60,
        source: "review",
        refId: "rl_1",
        at: new Date(NOW.getTime() - 86400000).toISOString(),
      },
    ];

    const trend = masteryTrendOf({
      kind: "vocabulary",
      states,
      events,
      attempts: [],
      days: 7,
      now: NOW,
    });

    expect(trend.current).toBe(60);
    expect(trend.series[trend.series.length - 1]?.value).toBe(60);
    // 昨天发生了 +20，因此"昨天结束"是 60，"前天结束"是 40
    expect(trend.series[trend.series.length - 2]?.value).toBe(60);
    expect(trend.series[trend.series.length - 3]?.value).toBe(40);
    expect(trend.delta).toBe(20);
  });

  it("记忆强度越高，实际留存越接近理论值之上", () => {
    const weak = forgettingCurveOf([entityState({ strength: 0.1 })]);
    const strong = forgettingCurveOf([
      entityState({ strength: 0.9 }),
      entityState({ entityId: "w_2", strength: 0.9 }),
    ]);

    const weakAt24 = weak.find((p) => p.hours === 24);
    const strongAt24 = strong.find((p) => p.hours === 24);
    expect(weakAt24 && strongAt24 && strongAt24.actual > weakAt24.actual).toBe(true);
  });

  it("掌握度分层统计", () => {
    const breakdown = masteryBreakdown([
      entityState({ state: "mastered" }),
      entityState({ state: "reviewing" }),
      entityState({ state: "new" }),
      entityState({ state: "lapsed" }),
    ]);

    expect(breakdown).toEqual({ mastered: 1, reviewing: 1, learning: 0, new: 1, lapsed: 1 });
  });
});
