import type { Difficulty, MasteryState, Vocabulary } from "./vocabulary";
import type { GrammarPoint } from "./grammar";
import type { Sentence } from "./sentence";
import type { EntityState } from "./learning-state";

export type KnowledgeKind = "word" | "phrase" | "grammar" | "sentence";

export const KNOWLEDGE_KIND_LABEL: Record<KnowledgeKind, string> = {
  word: "单词",
  phrase: "词组",
  grammar: "语法",
  sentence: "句子",
};

export type RelationKind =
  | "related"
  | "synonym"
  | "antonym"
  | "derived"
  | "used-in"
  | "explains";

export const RELATION_LABEL: Record<RelationKind, string> = {
  related: "相关",
  synonym: "近义",
  antonym: "反义",
  derived: "派生",
  "used-in": "用于",
  explains: "解释",
};

/** 图谱节点：只携带渲染所需的最小信息 + 指回原始实体的引用 */
export interface KnowledgeNode {
  id: string;
  kind: KnowledgeKind;
  label: string;
  /** 副标题：词性 / 语法分类 / 一句话释义 */
  sublabel: string;
  difficulty: Difficulty;
  mastery: number;
  state: MasteryState;
  /** 归一化坐标 0–1，由力导向布局预计算，保证界面稳定 */
  x: number;
  y: number;
  /** 权重用于节点半径 */
  weight: number;
}

export interface KnowledgeEdge {
  id: string;
  source: string;
  target: string;
  relation: RelationKind;
  weight: number;
}

export interface KnowledgeGraph {
  nodes: KnowledgeNode[];
  edges: KnowledgeEdge[];
  updatedAt: string;
}

export type MasteryFilter = "all" | MasteryState;
export type DifficultyFilter = "all" | Difficulty;
export type KindFilter = "all" | KnowledgeKind;

export interface KnowledgeFilter {
  keyword: string;
  kind: KindFilter;
  difficulty: DifficultyFilter;
  mastery: MasteryFilter;
}

export const DEFAULT_KNOWLEDGE_FILTER: KnowledgeFilter = {
  keyword: "",
  kind: "all",
  difficulty: "all",
  mastery: "all",
};

/** 任一知识实体的统一摘要，用于列表与详情抽屉 */
export interface KnowledgeSummary {
  id: string;
  kind: KnowledgeKind;
  primary: string;
  secondary: string;
  difficulty: Difficulty;
  mastery: number;
  state: MasteryState;
  tags: string[];
  dueAt: string | null;
}

/**
 * 详情用可辨识联合：UI 通过 switch(detail.kind) 拿到精确类型，
 * 不需要任何 any 或类型断言。
 */
export type KnowledgeDetail =
  | {
      kind: "word";
      word: Vocabulary;
      /** 该实体的学习态（唯一来源）；未学习过为 null */
      state: EntityState | null;
      relatedWords: KnowledgeSummary[];
      grammarPoints: GrammarPoint[];
      sentences: Sentence[];
    }
  | {
      kind: "phrase";
      word: Vocabulary;
      state: EntityState | null;
      relatedWords: KnowledgeSummary[];
      grammarPoints: GrammarPoint[];
      sentences: Sentence[];
    }
  | {
      kind: "grammar";
      grammar: GrammarPoint;
      state: EntityState | null;
      words: KnowledgeSummary[];
      sentences: Sentence[];
    }
  | {
      kind: "sentence";
      sentence: Sentence;
      state: EntityState | null;
      grammarPoints: GrammarPoint[];
      words: KnowledgeSummary[];
    };
