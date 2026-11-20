import type { MasteryState } from "./vocabulary";

export type LearningActivity =
  | "vocabulary"
  | "grammar"
  | "speaking"
  | "review"
  | "assessment"
  | "knowledge";

export const ACTIVITY_LABEL: Record<LearningActivity, string> = {
  vocabulary: "词汇训练",
  grammar: "语法训练",
  speaking: "口语训练",
  review: "复习",
  assessment: "水平测试",
  knowledge: "知识整理",
};

export interface LearningRecord {
  id: string;
  activity: LearningActivity;
  title: string;
  /** 学习分钟数 */
  minutes: number;
  /** 完成题目数 */
  items: number;
  correct: number;
  xp: number;
  occurredAt: string;
  /** 该记录涉及的实体引用，用于跳转知识库 */
  entityIds: string[];
}

export interface ReviewRecord {
  id: string;
  entityId: string;
  entityLabel: string;
  kind: "word" | "grammar" | "sentence";
  dueAt: string;
  /** 逾期天数，0 表示今天到期 */
  overdueDays: number;
  /** 记忆强度 0–1 */
  strength: number;
  state: MasteryState;
  lastReviewedAt: string | null;
}

export interface DailyStat {
  date: string;
  minutes: number;
  items: number;
  correct: number;
  xp: number;
  vocabularyReviewed: number;
  grammarReviewed: number;
}

export interface WeeklyStat {
  /** ISO 周序号标签，例如 "第 45 周" */
  label: string;
  weekStart: string;
  minutes: number;
  items: number;
  accuracy: number;
  activeDays: number;
}

export interface SeriesPoint {
  date: string;
  value: number;
}

export interface MasteryTrend {
  kind: "vocabulary" | "grammar" | "speaking";
  label: string;
  /** 当前掌握率 0–100 */
  current: number;
  /** 相对上一周期变化 */
  delta: number;
  series: SeriesPoint[];
}

export interface ForgettingCurvePoint {
  /** 学习后经过的小时数 */
  hours: number;
  /** 理论留存率 */
  expected: number;
  /** 用户实际留存率 */
  actual: number;
}

export interface TimeBucket {
  /** 时间段标签，例如 "06–09" */
  label: string;
  minutes: number;
  /** 该时段的正确率，用于双层映射 */
  accuracy: number;
}

export interface MasteryBreakdown {
  mastered: number;
  reviewing: number;
  learning: number;
  new: number;
  lapsed: number;
}

export interface StreakInfo {
  current: number;
  longest: number;
  /** 最近 14 天的活跃标记，用于 streak 条 */
  recent: { date: string; active: boolean; minutes: number }[];
}

export interface DashboardSnapshot {
  today: {
    minutes: number;
    goalMinutes: number;
    items: number;
    accuracy: number;
    xp: number;
  };
  streak: StreakInfo;
  totals: {
    minutes: number;
    words: number;
    masteredWords: number;
    grammarPoints: number;
    masteredGrammar: number;
    days: number;
  };
  mastery: {
    vocabulary: MasteryBreakdown;
    grammar: MasteryBreakdown;
  };
  vocabularyAccuracy: number;
  grammarAccuracy: number;
}
