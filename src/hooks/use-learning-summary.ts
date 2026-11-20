import { useMemo } from "react";
import { useLearningStore } from "@/stores/learning-store";
import { buildDailyStats, computeStreak } from "@/lib/algorithms/stats";
import { offsetMinutesForTimeZone } from "@/lib/date";
import { dueItems } from "@/services/data/selectors";

/**
 * 全局摘要（连续天数、到期复习数、今日进度）。
 * 只读，不发请求：数据已经在 store 里，避免侧边栏为了一枚徽章再拉一次接口。
 */
export function useLearningSummary() {
  const db = useLearningStore((state) => state.db);

  return useMemo(() => {
    const now = new Date();
    const daily = buildDailyStats({
      attempts: db.trainingAttempts,
      reviewLogs: db.reviewLogs,
      days: 30,
      now,
      offsetMinutes: offsetMinutesForTimeZone(db.profile.timezone, now),
    });

    return {
      streak: computeStreak(daily),
      dueCount: dueItems(db, now).length,
      todayMinutes: daily[daily.length - 1]?.minutes ?? 0,
      goalMinutes: db.profile.dailyMinutes,
      profile: db.profile,
    };
  }, [db]);
}
