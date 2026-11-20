import type { Difficulty } from "./vocabulary";

export type SentenceCategory =
  | "daily"
  | "academic"
  | "workplace"
  | "travel"
  | "opinion";

export const SENTENCE_CATEGORY_LABEL: Record<SentenceCategory, string> = {
  daily: "日常",
  academic: "学术",
  workplace: "职场",
  travel: "旅行",
  opinion: "观点表达",
};

export interface Sentence {
  id: string;
  en: string;
  zh: string;
  category: SentenceCategory;
  difficulty: Difficulty;
  level: string;
  /** 关联语法点 */
  grammarIds: string[];
  /** 关键词条 */
  wordIds: string[];
  /** 需要重点跟读的片段 */
  chunks: string[];
}
