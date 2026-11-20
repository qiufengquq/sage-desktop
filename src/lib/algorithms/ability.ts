import type {
  AbilityAxis,
  AbilityAxisKey,
  AbilityProfile,
  AssessmentAttempt,
  EntityState,
  ExerciseKind,
  TrainingAttempt,
  UserProfile,
} from "@/types";
import { average, clamp } from "@/lib/utils";

/**
 * 能力画像计算（确定性规则，不模拟 AI）。
 *
 * 数据来源分三类，界面必须如实标注：
 * - measured：该模块有测评记录，分数主要来自测评
 * - estimated：没有直接测评，由训练表现或知识库掌握度推算
 *
 * 六个轴的处理：
 *   vocabulary = 词汇测评 60% + 词条平均掌握度 40%
 *   grammar    = 语法测评 60% + 语法点平均掌握度 40%
 *   speaking   = 口语测评 70% + 口语训练正确率 30%
 *   listening  = 听写/跟读题正确率（估算）
 *   reading    = 完形/翻译题正确率（估算）
 *   writing    = 改写/翻译题正确率（估算）
 */

export const AXIS_LABEL: Record<AbilityAxisKey, string> = {
  vocabulary: "词汇量",
  grammar: "语法准确度",
  listening: "听力理解",
  speaking: "口语表达",
  reading: "阅读理解",
  writing: "写作",
};

/** 同级别参考均值，用于"高于/低于参考"的解释 */
const BENCHMARK: Record<AbilityAxisKey, number> = {
  vocabulary: 60,
  grammar: 58,
  listening: 62,
  speaking: 55,
  reading: 64,
  writing: 56,
};

/** 综合分权重：口语与词汇权重更高，符合"能说会用"的产品目标 */
const OVERALL_WEIGHTS: Record<AbilityAxisKey, number> = {
  vocabulary: 0.2,
  grammar: 0.2,
  listening: 0.15,
  speaking: 0.2,
  reading: 0.15,
  writing: 0.1,
};

/** 无数据时的基线（来自首次种子数据），避免雷达图出现 0 分 */
const FALLBACK: Record<AbilityAxisKey, number> = {
  vocabulary: 62,
  grammar: 54,
  listening: 58,
  speaking: 47,
  reading: 66,
  writing: 52,
};

const LEVEL_STEPS: { min: number; level: AbilityProfile["level"] }[] = [
  { min: 88, level: "C1" },
  { min: 75, level: "B2" },
  { min: 60, level: "B1" },
  { min: 45, level: "A2" },
  { min: 0, level: "A1" },
];

export function levelOfScore(score: number): AbilityProfile["level"] {
  return LEVEL_STEPS.find((step) => score >= step.min)?.level ?? "A1";
}

export interface AbilityInput {
  profile: UserProfile;
  assessmentAttempts: AssessmentAttempt[];
  trainingAttempts: TrainingAttempt[];
  states: Record<string, EntityState>;
  now: Date;
}

interface Accuracy {
  value: number | null;
  items: number;
}

export function buildAbilityProfile(input: AbilityInput): AbilityProfile {
  const { profile, assessmentAttempts, trainingAttempts, states, now } = input;

  const latestOf = (moduleId: AssessmentAttempt["moduleId"]) => {
    const list = assessmentAttempts
      .filter((a) => a.moduleId === moduleId)
      .sort((a, b) => new Date(b.finishedAt).getTime() - new Date(a.finishedAt).getTime());
    return list[0] ?? null;
  };

  const accuracyOfKinds = (kinds: ExerciseKind[]): Accuracy => {
    let correct = 0;
    let total = 0;
    for (const attempt of trainingAttempts) {
      for (const bucket of attempt.perKind) {
        if (!kinds.includes(bucket.kind)) continue;
        correct += bucket.correct;
        total += bucket.total;
      }
    }
    return { value: total === 0 ? null : Math.round((correct / total) * 100), items: total };
  };

  const entityStatesOf = (kinds: EntityState["kind"][]) =>
    Object.values(states).filter((s) => kinds.includes(s.kind));

  const avgMastery = (kinds: EntityState["kind"][]) => {
    const list = entityStatesOf(kinds);
    return list.length === 0 ? null : Math.round(average(list.map((s) => s.mastery)));
  };

  const vocabAssessment = latestOf("vocabulary");
  const grammarAssessment = latestOf("grammar");
  const speakingAssessment = latestOf("speaking");

  const wordMastery = avgMastery(["word", "phrase"]);
  const grammarMastery = avgMastery(["grammar"]);
  const listeningAccuracy = accuracyOfKinds(["dictation", "shadowing"]);
  const readingAccuracy = accuracyOfKinds(["cloze", "translate"]);
  const writingAccuracy = accuracyOfKinds(["rewrite", "translate"]);
  const speakingAccuracy = accuracyOfKinds(["shadowing", "roleplay"]);

  const axes: AbilityAxis[] = [
    buildAxis(
      "vocabulary",
      vocabAssessment?.score ?? null,
      wordMastery,
      "词汇测评",
      "词条平均掌握度",
    ),
    buildAxis(
      "grammar",
      grammarAssessment?.score ?? null,
      grammarMastery,
      "语法测评",
      "语法点平均掌握度",
    ),
    buildAxis("speaking", speakingAssessment?.score ?? null, speakingAccuracy.value, "口语测评", "口语训练正确率"),
    buildAxisFromAccuracy("listening", listeningAccuracy, "听写 / 跟读题正确率"),
    buildAxisFromAccuracy("reading", readingAccuracy, "完形 / 翻译题正确率"),
    buildAxisFromAccuracy("writing", writingAccuracy, "改写 / 翻译题正确率"),
  ];

  const overall = Math.round(
    axes.reduce((acc, axis) => acc + axis.score * OVERALL_WEIGHTS[axis.key], 0),
  );

  const ranked = [...axes].sort((a, b) => b.score - b.benchmark - (a.score - a.benchmark));
  const strengths = ranked
    .filter((axis) => axis.score >= axis.benchmark)
    .slice(0, 2)
    .map((axis) => `${axis.label} ${axis.score} 分，高于参考均值 ${axis.score - axis.benchmark} 分`);
  const weaknesses = ranked
    .filter((axis) => axis.score < axis.benchmark)
    .slice(-2)
    .reverse()
    .map(
      (axis) =>
        `${axis.label} ${axis.score} 分，低于参考均值 ${axis.benchmark - axis.score} 分（${
          axis.confidence === "measured" ? "测评结果" : "由训练表现估算"
        }）`,
    );

  return {
    level: levelOfScore(overall),
    targetLevel: profile.targetLevel,
    overall,
    axes,
    strengths: fillTo(strengths, "暂无高于参考均值的维度，先完成一次测评"),
    weaknesses: fillTo(weaknesses, "各维度接近参考均值，可以开始提高难度"),
    updatedAt: now.toISOString(),
    sourceAssessmentId:
      latestOf("vocabulary")?.id ?? latestOf("grammar")?.id ?? latestOf("speaking")?.id ?? null,
  };
}

function buildAxis(
  key: AbilityAxisKey,
  assessment: number | null,
  secondary: number | null,
  assessmentLabel: string,
  secondaryLabel: string,
): AbilityAxis {
  const fallback = FALLBACK[key];

  if (assessment !== null && secondary !== null) {
    const score = Math.round(assessment * 0.6 + secondary * 0.4);
    return axisOf(key, score, "measured", `${assessmentLabel} ${assessment} 分 × 60% + ${secondaryLabel} ${secondary} × 40%`);
  }
  if (assessment !== null) {
    return axisOf(key, assessment, "measured", `${assessmentLabel} ${assessment} 分`);
  }
  if (secondary !== null) {
    return axisOf(key, secondary, "estimated", `${secondaryLabel} ${secondary}（尚无测评）`);
  }
  return axisOf(key, fallback, "estimated", "基线估值，等待首次测评");
}

function buildAxisFromAccuracy(
  key: AbilityAxisKey,
  accuracy: Accuracy,
  label: string,
): AbilityAxis {
  if (accuracy.value === null || accuracy.items < 4) {
    return axisOf(key, FALLBACK[key], "estimated", `基线估值（${label}样本不足）`);
  }
  return axisOf(key, accuracy.value, "estimated", `${label} ${accuracy.value}%（${accuracy.items} 题）`);
}

function axisOf(
  key: AbilityAxisKey,
  score: number,
  confidence: AbilityAxis["confidence"],
  source: string,
): AbilityAxis {
  const clamped = clamp(Math.round(score), 0, 100);
  return {
    key,
    label: AXIS_LABEL[key],
    score: clamped,
    benchmark: BENCHMARK[key],
    level: levelOfScore(clamped),
    confidence,
    source,
  };
}

function fillTo(items: string[], placeholder: string): string[] {
  if (items.length >= 2) return items.slice(0, 2);
  return [...items, ...Array.from({ length: 2 - items.length }, () => placeholder)];
}
