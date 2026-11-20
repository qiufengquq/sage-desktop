import { describe, expect, it } from "vitest";

import { buildDailyStats } from "@/lib/algorithms/stats";
import {
  daysBetweenIn,
  hourIn,
  isoDateIn,
  lastNDaysIn,
  offsetMinutesForTimeZone,
  startOfDayIn,
} from "@/lib/date";

/**
 * 2.2.5 的回归护栏：归日与小时分布必须按用户时区，而不是运行环境时区。
 *
 * 之前统计直接调用 `new Date(iso).getHours()` / `setHours(0,0,0,0)`，
 * 于是同一份学习记录在不同设备/时区下会被算到不同的一天。
 */
const SHANGHAI = 480; // UTC+8
const NEW_YORK_WINTER = -300; // UTC-5（11 月为 EST）

describe("时区偏移换算", () => {
  const at = new Date("2026-11-14T17:30:00.000Z");

  it("IANA 时区换算成分钟偏移", () => {
    expect(offsetMinutesForTimeZone("Asia/Shanghai", at)).toBe(SHANGHAI);
    expect(offsetMinutesForTimeZone("America/New_York", at)).toBe(NEW_YORK_WINTER);
  });

  it("无法识别的时区回退到本地偏移而不抛错", () => {
    const fallback = offsetMinutesForTimeZone("Not/AZone", at);
    expect(Number.isFinite(fallback)).toBe(true);
  });

  it("同一时刻在不同时区属于不同的自然日", () => {
    expect(isoDateIn(at, SHANGHAI)).toBe("2026-11-15");
    expect(isoDateIn(at, NEW_YORK_WINTER)).toBe("2026-11-14");
  });

  it("小时按给定时区换算", () => {
    expect(hourIn(at, SHANGHAI)).toBe(1); // 次日 01:30
    expect(hourIn(at, NEW_YORK_WINTER)).toBe(12); // 当日 12:30
  });

  it("当天零点与日差计算自洽", () => {
    expect(startOfDayIn(at, SHANGHAI).toISOString()).toBe("2026-11-14T16:00:00.000Z");
    // at 在东八区是 11-15 01:30；下面这个时刻在东八区是 11-16 10:00
    expect(daysBetweenIn(at, "2026-11-16T02:00:00.000Z", SHANGHAI)).toBe(1);
    expect(lastNDaysIn(3, at, SHANGHAI)).toEqual(["2026-11-13", "2026-11-14", "2026-11-15"]);
  });
});

describe("统计窗口按用户时区滚动", () => {
  const now = new Date("2026-11-14T17:30:00.000Z");

  it("同一个 now 在不同时区得到不同的日期窗口", () => {
    const shanghai = buildDailyStats({
      attempts: [],
      reviewLogs: [],
      days: 3,
      now,
      offsetMinutes: SHANGHAI,
    });
    const newYork = buildDailyStats({
      attempts: [],
      reviewLogs: [],
      days: 3,
      now,
      offsetMinutes: NEW_YORK_WINTER,
    });

    expect(shanghai.map((day) => day.date)).toEqual(["2026-11-13", "2026-11-14", "2026-11-15"]);
    expect(newYork.map((day) => day.date)).toEqual(["2026-11-12", "2026-11-13", "2026-11-14"]);
    // 窗口最后一天永远是"用户时区的今天"
    expect(shanghai[shanghai.length - 1]?.date).toBe(isoDateIn(now, SHANGHAI));
    expect(newYork[newYork.length - 1]?.date).toBe(isoDateIn(now, NEW_YORK_WINTER));
  });
});
