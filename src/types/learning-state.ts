import type { ExerciseAnswer, ExerciseKind, KindScore, TrainingTrack } from "./exercise";
import type { AppSettings, UserProfile } from "./user";
import type { Difficulty, MasteryState } from "./vocabulary";
import type { StudyPlan } from "./plan";

/**
 * 统一的业务数据模型。
 *
 * 设计原则：
 * 1. 内容态（词条/语法/句子的正文）只读，学习态（掌握度等）单独存放，便于未来落到 SQLite。
 * 2. 历史事件（训练/测评/复习）只追加，不覆盖；派生指标（每日统计、连续天数、趋势）一律现算。
 * 3. 每条事件都有 `submissionId`，用于重复提交时的幂等判定。
 */

export type EntityKind = "word" | "phrase" | "grammar" | "sentence";

/** 单个知识对象的学习态，主键与实体 id 相同 */
export interface EntityState {
  entityId: string;
  kind: EntityKind;
  /** 掌握度 0–100 */
  mastery: number;
  /** 记忆强度 0–1，用于遗忘模型与复习排序 */
  strength: number;
  state: MasteryState;
  /** 累计复习/作答次数 */
  reviewCount: number;
  /** 遗忘次数（作答明显错误或复习标记"忘记"） */
  lapseCount: number;
  /** SM-2 的难度因子，1.3–2.8 */
  ease: number;
  /** SM-2 的连续成功次数，失败归零 */
  repetition: number;
  /** 当前复习间隔（天），0 表示处于重新学习阶段 */
  intervalDays: number;
  dueAt: string | null;
  lastReviewedAt: string | null;
  updatedAt: string;
}

/**
 * 种子数据里的"初始学习态提示"。
 *
 * 它**不是**运行时模型：只在首次建库时把 demo 数据变成 `EntityState`。
 * 单独成类型是为了让内容实体保持纯内容，避免再次把掌握度写回 `Vocabulary` 等类型。
 */
export interface EntityStateSeed {
  entityId: string;
  kind: EntityKind;
  mastery: number;
  strength: number;
  state: MasteryState;
  reviewCount: number;
  dueAt: string | null;
  lastReviewedAt: string | null;
  addedAt: string | null;
}

/** 一次训练的完整记录（含逐题作答，可追溯） */
export interface TrainingAttempt {
  id: string;
  /** 幂等键：同一会话重复提交不会重复记分 */
  submissionId: string;
  sessionId: string;
  kind: ExerciseKind;
  track: TrainingTrack;
  title: string;
  /** 训练难度 1–5，取题时使用 */
  difficulty: Difficulty;
  minutes: number;
  durationMs: number;
  score: number;
  maxScore: number;
  accuracy: number;
  xp: number;
  perKind: KindScore[];
  answers: ExerciseAnswer[];
  /** 本次涉及的知识实体，用于掌握度回写与跳转 */
  entityIds: string[];
  startedAt: string;
  finishedAt: string;
}

/** 一次测评的完整记录 */
export interface AssessmentAttempt {
  id: string;
  submissionId: string;
  moduleId: "vocabulary" | "grammar" | "speaking";
  score: number;
  accuracy: number;
  level: string;
  perSection: {
    sectionId: string;
    title: string;
    correct: number;
    total: number;
    score: number;
  }[];
  answers: ExerciseAnswer[];
  strengths: string[];
  weaknesses: string[];
  recommendation: string;
  minutes: number;
  finishedAt: string;
}

/**
 * 掌握度变化事件。
 * 训练与复习都会改掌握度，这里逐条记录 before/after，
 * 保证趋势图可以精确回放，而不是靠当前值倒推。
 */
export interface MasteryEvent {
  id: string;
  entityId: string;
  kind: EntityKind;
  before: number;
  after: number;
  source: "training" | "review" | "assessment";
  /** 关联的训练会话或复习条目，便于追溯 */
  refId: string;
  at: string;
}

/** 一次复习动作（"认识 / 模糊 / 忘记"） */
export interface ReviewLog {
  id: string;
  entityId: string;
  rating: "again" | "hard" | "good" | "easy";
  /** 复习前的到期时间，便于追溯调度是否合理 */
  prevDueAt: string | null;
  nextDueAt: string;
  prevIntervalDays: number;
  nextIntervalDays: number;
  ease: number;
  masteryBefore: number;
  masteryAfter: number;
  xp: number;
  at: string;
}

/** 计划是怎么来的，避免"看起来像 AI 生成的" */
export interface PlanGenerationMeta {
  generatedBy: "seed" | "regenerate" | "assessment" | "scheduled";
  generatedAt: string;
  /** 用户手动确认过的难度；null 表示沿用建议值 */
  difficultyOverride: number | null;
}

/** 持久化文档（未来对应 SQLite 的一组表） */
export interface LearningDb {
  schemaVersion: number;
  profile: UserProfile;
  settings: AppSettings;
  entityStates: Record<string, EntityState>;
  trainingAttempts: TrainingAttempt[];
  assessmentAttempts: AssessmentAttempt[];
  reviewLogs: ReviewLog[];
  masteryEvents: MasteryEvent[];
  plan: StudyPlan | null;
  planMeta: PlanGenerationMeta;
  /** 首次写入种子数据的时间；null 表示尚未种子化 */
  seededAt: string | null;
}
