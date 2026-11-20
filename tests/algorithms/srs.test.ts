import { describe, expect, it } from "vitest";
import {
  initialSchedulerState,
  isDue,
  overdueDaysOf,
  scheduleNext,
  SRS_PARAMS,
} from "@/lib/algorithms/srs";
import { NOW } from "../helpers/fixtures";

const DAY = 86400000;

describe("复习调度（SM-2 变体）", () => {
  it("初次学习：认识后 1 天复习", () => {
    const result = scheduleNext(initialSchedulerState(), "good", NOW);
    expect(result.intervalDays).toBe(1);
    expect(result.repetition).toBe(1);
    expect(new Date(result.dueAt).getTime()).toBe(NOW.getTime() + DAY);
  });

  it("忘记后进入重学，10 分钟后再出现", () => {
    const result = scheduleNext(
      { ease: 2.5, intervalDays: 10, repetition: 3, lapseCount: 0 },
      "again",
      NOW,
    );
    expect(result.intervalDays).toBe(0);
    expect(result.repetition).toBe(0);
    expect(result.lapseCount).toBe(1);
    expect(new Date(result.dueAt).getTime()).toBe(NOW.getTime() + SRS_PARAMS.relearnMinutes * 60000);
    expect(result.ease).toBeCloseTo(2.3, 5);
  });

  it("第二次成功：间隔延长到 3 天", () => {
    const result = scheduleNext({ ease: 2.5, intervalDays: 1, repetition: 1, lapseCount: 0 }, "good", NOW);
    expect(result.intervalDays).toBe(3);
    expect(result.repetition).toBe(2);
  });

  it("连续成功后按难度因子增长", () => {
    const result = scheduleNext({ ease: 2.5, intervalDays: 10, repetition: 3, lapseCount: 0 }, "good", NOW);
    expect(result.intervalDays).toBe(25);
    expect(result.dueAt).toBe(new Date(NOW.getTime() + 25 * DAY).toISOString());
  });

  it("很熟的间隔比认识更长", () => {
    const base = { ease: 2.5, intervalDays: 10, repetition: 3, lapseCount: 0 };
    const good = scheduleNext(base, "good", NOW);
    const easy = scheduleNext(base, "easy", NOW);
    expect(easy.intervalDays).toBeGreaterThan(good.intervalDays);
  });

  it("模糊时按 1.2 倍增长并降低难度因子", () => {
    const result = scheduleNext({ ease: 2.5, intervalDays: 10, repetition: 3, lapseCount: 0 }, "hard", NOW);
    expect(result.intervalDays).toBe(12);
    expect(result.ease).toBeCloseTo(2.35, 5);
  });

  it("难度因子限制在 1.3–2.8", () => {
    let state = { ease: 2.5, intervalDays: 5, repetition: 2, lapseCount: 0 };
    for (let i = 0; i < 12; i += 1) {
      const result = scheduleNext(state, "again", NOW);
      state = {
        ease: result.ease,
        intervalDays: 0,
        repetition: 0,
        lapseCount: result.lapseCount,
      };
    }
    expect(state.ease).toBe(SRS_PARAMS.minEase);

    let rising = { ease: 2.5, intervalDays: 5, repetition: 3, lapseCount: 0 };
    for (let i = 0; i < 6; i += 1) {
      const result = scheduleNext(rising, "easy", NOW);
      rising = {
        ease: result.ease,
        intervalDays: result.intervalDays,
        repetition: result.repetition,
        lapseCount: result.lapseCount,
      };
    }
    expect(rising.ease).toBe(SRS_PARAMS.maxEase);
  });

  it("间隔上限 180 天", () => {
    const result = scheduleNext({ ease: 2.8, intervalDays: 170, repetition: 8, lapseCount: 0 }, "easy", NOW);
    expect(result.intervalDays).toBe(SRS_PARAMS.maxIntervalDays);
  });

  it("到期判断与逾期天数使用本地自然日", () => {
    const yesterday = new Date(NOW.getTime() - DAY).toISOString();
    const tomorrow = new Date(NOW.getTime() + DAY).toISOString();

    expect(isDue(yesterday, NOW)).toBe(true);
    expect(isDue(tomorrow, NOW)).toBe(false);
    expect(overdueDaysOf(yesterday, NOW)).toBe(1);
    expect(overdueDaysOf(tomorrow, NOW)).toBe(-1);
  });
});
