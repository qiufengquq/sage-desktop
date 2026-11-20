import type {
  DailyStat,
  DashboardSnapshot,
  ForgettingCurvePoint,
  LearningActivity,
  LearningRecord,
  MasteryTrend,
  ReviewRecord,
  StreakInfo,
  TimeBucket,
  WeeklyStat,
} from "./learning";
import type { PlanAdvice, PlanItemAction, PlanItemStatus } from "./plan";
import type { TrainingOption } from "./exercise";

export interface DashboardTask {
  id: string;
  title: string;
  activity: LearningActivity;
  minutes: number;
  xp: number;
  /** 为什么会出现这条任务，用于建立可解释性 */
  reason: string;
  status: PlanItemStatus;
  /** 点击任务时的目标：复习 / 训练 / 知识库 */
  action: PlanItemAction;
}

export interface DashboardData {
  snapshot: DashboardSnapshot;
  tasks: DashboardTask[];
  reviewTasks: ReviewRecord[];
  recentRecords: LearningRecord[];
  recommendation: PlanAdvice | null;
  quickStart: TrainingOption[];
  /** 今日计划完成率 0–100 */
  planCompletion: number;
  /** 当前时刻在一天中的位置 0–1，用于时间轴定位 */
  nowProgress: number;
}

export interface AnalyticsTotals {
  minutes: number;
  items: number;
  accuracy: number;
  activeDays: number;
  avgMinutes: number;
  bestDay: { date: string; minutes: number } | null;
}

export interface AnalyticsData {
  range: 7 | 14 | 30;
  daily: DailyStat[];
  weekly: WeeklyStat[];
  trends: MasteryTrend[];
  forgettingCurve: ForgettingCurvePoint[];
  timeDistribution: TimeBucket[];
  reviewQueue: ReviewRecord[];
  mastered: ReviewRecord[];
  totals: AnalyticsTotals;
  streak: StreakInfo;
}
