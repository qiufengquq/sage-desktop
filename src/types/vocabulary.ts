export type PartOfSpeech =
  | "noun"
  | "verb"
  | "adjective"
  | "adverb"
  | "preposition"
  | "conjunction"
  | "pronoun"
  | "phrase";

/** 难度 1–5，与 CEFR 粗略对应：1≈A1 … 5≈C1+ */
export type Difficulty = 1 | 2 | 3 | 4 | 5;

/** 单个知识对象的学习状态机 */
export type MasteryState = "new" | "learning" | "reviewing" | "mastered" | "lapsed";

export interface WordMeaning {
  pos: PartOfSpeech;
  /** 中文释义 */
  definition: string;
  /** 英文释义，用于沉浸式复习 */
  definitionEn?: string;
}

export interface WordExample {
  id: string;
  en: string;
  zh: string;
  /** 例句中的高亮片段 */
  highlight?: string;
}

export interface Vocabulary {
  id: string;
  term: string;
  phonetic: string;
  pos: PartOfSpeech;
  difficulty: Difficulty;
  level: string;
  meanings: WordMeaning[];
  examples: WordExample[];
  /** 相关词汇（同义/反义/同根） */
  relatedWordIds: string[];
  /** 关联语法点 */
  grammarIds: string[];
  /** 使用场景标签，例如 "academic"、"daily" */
  contexts: string[];
  addedAt: string;
}

export const PART_OF_SPEECH_LABEL: Record<PartOfSpeech, string> = {
  noun: "名词",
  verb: "动词",
  adjective: "形容词",
  adverb: "副词",
  preposition: "介词",
  conjunction: "连词",
  pronoun: "代词",
  phrase: "词组",
};

export const MASTERY_STATE_LABEL: Record<MasteryState, string> = {
  new: "未学习",
  learning: "学习中",
  reviewing: "复习中",
  mastered: "已掌握",
  lapsed: "已遗忘",
};

export const DIFFICULTY_LABEL: Record<Difficulty, string> = {
  1: "入门",
  2: "基础",
  3: "进阶",
  4: "高阶",
  5: "精通",
};
