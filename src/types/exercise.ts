import type { Difficulty } from "./vocabulary";

export type ExerciseKind =
  | "word-match"
  | "multiple-choice"
  | "spelling"
  | "dictation"
  | "cloze"
  | "translate"
  | "sentence-order"
  | "error-correction"
  | "rewrite"
  | "shadowing"
  | "roleplay";

export type TrainingTrack = "vocabulary" | "grammar" | "speaking";

export interface ExercisePrompt {
  /** 面向用户的指令，主动语态 */
  instruction: string;
  /** 题干补充上下文 */
  context?: string;
}

export interface ExerciseOption {
  id: string;
  label: string;
  /**
   * 是否为正确选项。
   *
   * **后端下发的题目不含此字段**（判分在服务端）；只有本地题库（mock 模式）
   * 与内容包导出时才有。任何展示逻辑都必须以 `AnswerCheck` 为准。
   */
  correct?: boolean;
}

export interface MatchPair {
  id: string;
  left: string;
  right: string;
}

export interface RoleplayTurn {
  id: string;
  speaker: "ai" | "user";
  en: string;
  zh: string;
  /** 用户轮次的可接受表达，用于模拟评分 */
  expected?: string[];
}

interface ExerciseBase {
  id: string;
  prompt: ExercisePrompt;
  difficulty: Difficulty;
  /** 关联知识实体，训练后可回写掌握度 */
  entityIds: string[];
  explanation: string;
  xp: number;
  tags: string[];
}

export interface MatchExercise extends ExerciseBase {
  kind: "word-match";
  /** 正确配对。后端下发时被移除，只保留 lefts / rights */
  pairs?: MatchPair[];
  /** 左列素材（后端下发的展示字段） */
  lefts?: string[];
  /** 右列素材（后端下发的展示字段，未配对） */
  rights?: string[];
}

export interface ChoiceExercise extends ExerciseBase {
  kind: "multiple-choice";
  stem: string;
  options: ExerciseOption[];
}

export interface SpellingExercise extends ExerciseBase {
  kind: "spelling";
  /** 中文释义提示 */
  definition: string;
  /** 正确答案：仅本地题库/内容包有，后端下发时被移除 */
  answer?: string;
  hint: string;
}

export interface DictationExercise extends ExerciseBase {
  kind: "dictation";
  /** 朗读文本，未来由 TTS 生成音频，此处用于降级显示 */
  audioText: string;
  answer?: string;
  hint: string;
}

export interface ClozeExercise extends ExerciseBase {
  kind: "cloze";
  /** 含 {{blank}} 占位符的句子 */
  sentence: string;
  options: ExerciseOption[];
  answer?: string;
}

export interface TranslateExercise extends ExerciseBase {
  kind: "translate";
  source: string;
  sourceLang: "zh" | "en";
  targetLang: "zh" | "en";
  answer?: string;
  /** 参考译文，允许多解 */
  references?: string[];
}

export interface OrderExercise extends ExerciseBase {
  kind: "sentence-order";
  /** 打乱的词块 */
  tokens: string[];
  answer?: string[];
}

export interface CorrectionExercise extends ExerciseBase {
  kind: "error-correction";
  wrong: string;
  answer?: string;
  errorTag: string;
}

export interface RewriteExercise extends ExerciseBase {
  kind: "rewrite";
  source: string;
  instruction: string;
  answer?: string;
  references?: string[];
}

export interface ShadowingExercise extends ExerciseBase {
  kind: "shadowing";
  text: string;
  translation: string;
  /** 逐词分块，用于界面标注节奏 */
  chunks: string[];
  targetSeconds: number;
}

export interface RoleplayExercise extends ExerciseBase {
  kind: "roleplay";
  scenario: string;
  turns: RoleplayTurn[];
}

export type Exercise =
  | MatchExercise
  | ChoiceExercise
  | SpellingExercise
  | DictationExercise
  | ClozeExercise
  | TranslateExercise
  | OrderExercise
  | CorrectionExercise
  | RewriteExercise
  | ShadowingExercise
  | RoleplayExercise;

export interface ExerciseAnswer {
  exerciseId: string;
  /** 字符串答案、排序数组，或连线映射 */
  value: string | string[] | Record<string, string>;
  correct: boolean;
  /** 0–1，口语与翻译题使用部分给分 */
  partialScore: number;
  elapsedMs: number;
}

export interface KindScore {
  kind: ExerciseKind;
  correct: number;
  total: number;
}

/**
 * 逐题即时反馈的结果（由服务端裁定）。
 *
 * 判分权收归后端后，客户端不再持有正确答案，因此
 * "这题对不对 / 正确答案是什么 / 讲解" 必须由这次校验回传。
 * mock 模式下由本地判分产生同形结果，页面无需区分来源。
 */
export interface AnswerCheck {
  exerciseId: string;
  correct: boolean;
  partialScore: number;
  /** 按题型不同：字符串 / 字符串数组 / 配对列表 */
  correctAnswer: unknown;
  explanation: string;
}

export interface ExerciseResult {
  sessionId: string;
  track: TrainingTrack;
  answers: ExerciseAnswer[];
  score: number;
  maxScore: number;
  accuracy: number;
  durationMs: number;
  xp: number;
  perKind: KindScore[];
  /** 需要复习的知识实体 */
  weakEntityIds: string[];
  finishedAt: string;
  /** 是否为重复提交（同一会话已计过分） */
  duplicate?: boolean;
}

export interface TrainingSession {
  id: string;
  track: TrainingTrack;
  kind: ExerciseKind;
  title: string;
  exercises: Exercise[];
  startedAt: string;
  estimatedMinutes: number;
  targetEntityIds: string[];
}

export interface TrainingOption {
  id: string;
  track: TrainingTrack;
  kind: ExerciseKind;
  title: string;
  description: string;
  icon: string;
  minutes: number;
  itemCount: number;
  difficulty: Difficulty;
  /** 是否已有进度 */
  progress: number;
}
