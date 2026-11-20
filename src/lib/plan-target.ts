import type { ExerciseKind, PlanItemAction, TrainingTrack } from "@/types";

/** 计划项 / 仪表盘任务 → 可导航目标，避免各处重复拼 URL */

export interface TrainingTarget {
  kind: ExerciseKind;
  track: TrainingTrack;
}

export function trainingTargetOf(action: PlanItemAction): TrainingTarget | null {
  return action.type === "training" ? { kind: action.kind, track: action.track } : null;
}

export function trainingRoute(target: TrainingTarget): string {
  return `/training?kind=${encodeURIComponent(target.kind)}&track=${encodeURIComponent(
    target.track,
  )}&start=1`;
}

export function planActionRoute(action: PlanItemAction): string {
  if (action.type === "training") return trainingRoute({ kind: action.kind, track: action.track });
  if (action.type === "review") {
    return action.entityId ? `/review?entity=${encodeURIComponent(action.entityId)}` : "/review";
  }
  return `/knowledge?id=${encodeURIComponent(action.entityId)}`;
}

const KIND_LABEL: Partial<Record<ExerciseKind, string>> = {
  "word-match": "单词连线",
  "multiple-choice": "选择",
  spelling: "拼写",
  dictation: "听写",
  cloze: "填空",
  translate: "翻译",
  "sentence-order": "句子排序",
  "error-correction": "语法纠错",
  rewrite: "句子改写",
  shadowing: "跟读",
  roleplay: "情景对话",
};

export function planActionLabel(action: PlanItemAction): string {
  if (action.type === "review") return "复习";
  if (action.type === "knowledge") return "知识库";
  return KIND_LABEL[action.kind] ?? action.kind;
}
