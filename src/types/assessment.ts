import type { Exercise, ExerciseKind, TrainingTrack } from "./exercise";

export type AssessmentModuleId = "vocabulary" | "grammar" | "speaking";

export interface AssessmentSection {
  id: string;
  title: string;
  description: string;
  kind: ExerciseKind;
  /** 该分项的考察维度，用于结果面板解释 */
  skill: string;
  items: Exercise[];
}

export interface AssessmentModule {
  id: AssessmentModuleId;
  title: string;
  subtitle: string;
  description: string;
  minutes: number;
  sections: AssessmentSection[];
  /** 最近一次完成情况 */
  lastAttempt: { at: string; score: number } | null;
}

export interface SectionOutcome {
  sectionId: string;
  title: string;
  correct: number;
  total: number;
  score: number;
}

export interface AssessmentOutcome {
  moduleId: AssessmentModuleId;
  score: number;
  accuracy: number;
  level: string;
  perSection: SectionOutcome[];
  strengths: string[];
  weaknesses: string[];
  recommendation: string;
  finishedAt: string;
  /** "去训练"应该去哪里：指向本次最弱分项对应的训练入口 */
  followUp: { label: string; kind: ExerciseKind; track: TrainingTrack } | null;
  /** 是否为重复提交（命中 submissionId 幂等键） */
  duplicate?: boolean;
}
