import type { CefrLevel } from "./user";
import type { LearningActivity } from "./learning";
import type { ExerciseKind, TrainingTrack } from "./exercise";

export type AbilityAxisKey =
  | "vocabulary"
  | "grammar"
  | "listening"
  | "speaking"
  | "reading"
  | "writing";

export interface AbilityAxis {
  key: AbilityAxisKey;
  label: string;
  /** 0–100 */
  score: number;
  /** 同水平参考均值 */
  benchmark: number;
  level: CefrLevel;
  /**
   * 数据来源可信度：
   * - measured：本模块有测评结果，分数来自真实作答
   * - estimated：没有直接测评，由训练表现或基线推算
   * 界面上必须如实标注，不能把估算值说成测评结果。
   */
  confidence: "measured" | "estimated";
  /** 这个分数是如何算出来的，供 UI 展开说明 */
  source: string;
}

export interface AbilityProfile {
  level: CefrLevel;
  targetLevel: CefrLevel;
  /** 综合分 0–100 */
  overall: number;
  axes: AbilityAxis[];
  /** 两项优势 */
  strengths: string[];
  /** 两项短板 */
  weaknesses: string[];
  updatedAt: string;
  /** 画像由哪次测试生成 */
  sourceAssessmentId: string | null;
}

export type PlanItemStatus = "pending" | "in-progress" | "done" | "skipped";

/** 计划项点击后应该去哪里：复习队列 / 训练入口 / 知识库条目 */
export type PlanItemAction =
  | { type: "review"; entityId?: string }
  | { type: "training"; kind: ExerciseKind; track: TrainingTrack }
  | { type: "knowledge"; entityId: string };

export interface PlanItem {
  id: string;
  title: string;
  activity: LearningActivity;
  /** 计划分钟数 */
  minutes: number;
  /** 关联入口，决定按钮行为 */
  action: PlanItemAction;
  done: boolean;
  status: PlanItemStatus;
  /** 预计获得的经验 */
  xp: number;
  reason: string;
}

export interface PlanDay {
  id: string;
  /** ISO 日期 */
  date: string;
  focus: string;
  minutes: number;
  items: PlanItem[];
  completed: boolean;
}

export interface WeeklyGoal {
  id: string;
  label: string;
  target: number;
  current: number;
  unit: "分钟" | "天" | "词" | "题";
  /** 该目标为何被设定 */
  rationale: string;
}

export interface PlanAdvice {
  id: string;
  kind: "vocabulary" | "grammar" | "speaking";
  title: string;
  body: string;
  /** 依据：必须来自真实数据，可被用户逐条核对 */
  evidence: string[];
  /** 可执行动作 */
  action: { label: string; trainingKind: ExerciseKind; track: TrainingTrack };
}

export interface DifficultyAdjustment {
  current: number;
  suggested: number;
  reason: string;
  /** 是否等待用户确认才应用 */
  requiresConfirmation: boolean;
}

export interface StudyPlan {
  id: string;
  profile: AbilityProfile;
  today: PlanDay;
  week: PlanDay[];
  weeklyGoals: WeeklyGoal[];
  advice: PlanAdvice[];
  difficulty: DifficultyAdjustment;
  completion: { done: number; total: number; rate: number };
  generatedAt: string;
  /** 模型标识，为真实 AI 接入预留 */
  model: string;
}
