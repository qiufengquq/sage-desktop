import type { EntityKind } from "./learning-state";
import type { Difficulty, MasteryState } from "./vocabulary";

/** 复习时的自评等级，决定下一次复习间隔 */
export type ReviewRating = "again" | "hard" | "good" | "easy";

export const REVIEW_RATING_LABEL: Record<ReviewRating, string> = {
  again: "忘记",
  hard: "模糊",
  good: "认识",
  easy: "很熟",
};

/** 复习队列中的一项（由到期时间与掌握度派生） */
export interface ReviewQueueItem {
  entityId: string;
  kind: EntityKind;
  /** 主展示文本：单词 / 语法标题 / 英文句子 */
  label: string;
  /** 副展示文本：释义 / 英文名 / 中文翻译 */
  secondary: string;
  difficulty: Difficulty;
  mastery: number;
  strength: number;
  state: MasteryState;
  dueAt: string;
  /** 逾期天数，0 表示今天到期，负数表示提前 */
  overdueDays: number;
  lastReviewedAt: string | null;
  reviewCount: number;
}

/** 复习卡片内容：先回忆，再揭晓 */
export interface ReviewCard {
  item: ReviewQueueItem;
  /** 回忆阶段展示的提示 */
  prompt: string;
  /** 揭晓后的答案主体 */
  answer: string;
  /** 例句 / 结构 / 用法等补充信息 */
  detail: string[];
  /** 复习后应产生的知识点说明，用于 UI 解释掌握度变化 */
  focus: string;
  /** 四种自评分别会把下次复习排到多久之后，让用户做决定前就能看到 */
  preview: Record<ReviewRating, string>;
}

export interface ReviewSummary {
  reviewed: number;
  again: number;
  hard: number;
  good: number;
  easy: number;
  xp: number;
  /** 复习后仍处于到期状态的条目数 */
  remaining: number;
  /** 下次最近到期时间 */
  nextDueAt: string | null;
  durationMs: number;
}
