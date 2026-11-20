import type { Difficulty } from "./vocabulary";

export type GrammarCategory =
  | "tense"
  | "clause"
  | "preposition"
  | "article"
  | "sentence-structure"
  | "modality"
  | "voice";

export const GRAMMAR_CATEGORY_LABEL: Record<GrammarCategory, string> = {
  tense: "时态",
  clause: "从句",
  preposition: "介词",
  article: "冠词",
  "sentence-structure": "句子结构",
  modality: "情态",
  voice: "语态",
};

export interface GrammarExample {
  id: string;
  en: string;
  zh: string;
  note?: string;
}

export interface GrammarMistake {
  id: string;
  wrong: string;
  correct: string
  reason: string;
}

export interface GrammarPoint {
  id: string;
  title: string;
  titleEn: string;
  category: GrammarCategory;
  difficulty: Difficulty;
  level: string;
  /** 一句话核心规则 */
  summary: string;
  /** 结构公式，例如 "主语 + have/has + 过去分词" */
  structure: string;
  explanation: string;
  examples: GrammarExample[];
  mistakes: GrammarMistake[];
  relatedWordIds: string[];
}
