import type {
  AssessmentAttempt,
  EntityState,
  ExerciseAnswer,
  LearningDb,
  MasteryEvent,
  TrainingAttempt,
  TrainingTrack,
} from "@/types";
import { grammarStateHints } from "@/services/mock/grammar";
import { sentenceStateHints } from "@/services/mock/sentences";
import { wordStateHints } from "@/services/mock/vocabulary";
import { learningRecords } from "@/services/mock/learning";
import { assessmentModules } from "@/services/mock/assessment";
import { studyPlan } from "@/services/mock/planner";
import { defaultSettings, userProfile } from "@/services/mock/user";
import { addDays } from "@/lib/date";

/**
 * 首次启动的种子数据。
 *
 * 用途：让第一屏不是空图。所有数值都来自第一阶段已有的 mock 实体，
 * 不做额外编造；训练/测评历史由第一阶段的学习记录转换而来，
 * 在 DATA_MODEL.md 中标注为"示例历史"。
 */

export const SCHEMA_VERSION = 1;

export function buildSeedDb(now: Date): LearningDb {
  const entityStates = buildEntityStates(now);
  const trainingAttempts = buildSeedAttempts();

  return {
    schemaVersion: SCHEMA_VERSION,
    profile: { ...userProfile },
    settings: structuredClone(defaultSettings),
    entityStates,
    trainingAttempts,
    assessmentAttempts: buildSeedAssessments(),
    reviewLogs: [],
    masteryEvents: buildSeedMasteryEvents(entityStates),
    plan: seedPlan(),
    planMeta: {
      generatedBy: "seed",
      generatedAt: now.toISOString(),
      difficultyOverride: null,
    },
    seededAt: now.toISOString(),
  };
}

/** 种子计划来自第一阶段的示例数据，但完成率必须与项目状态一致 */
function seedPlan() {
  const plan = structuredClone(studyPlan);
  const done = plan.today.items.filter((item) => item.done).length;
  const total = plan.today.items.length;
  return {
    ...plan,
    completion: {
      done,
      total,
      rate: Math.round((done / Math.max(1, total)) * 100),
    },
    today: { ...plan.today, completed: done === total },
  };
}

/**
 * 由"种子提示"构造初始学习态。
 *
 * 注意：内容实体（Vocabulary/GrammarPoint/Sentence）**不再**携带掌握度字段，
 * 这里读取的是 mock 层单独导出的初始态种子，属于首次建库的一次性输入。
 */
function buildEntityStates(now: Date): Record<string, EntityState> {
  const states: Record<string, EntityState> = {};
  const hints = [...wordStateHints, ...grammarStateHints, ...sentenceStateHints];
  const fallbackUpdatedAt = addDays(now, -30).toISOString();

  for (const hint of hints) {
    states[hint.entityId] = {
      entityId: hint.entityId,
      kind: hint.kind,
      mastery: hint.mastery,
      strength: hint.strength,
      state: hint.state,
      reviewCount: hint.reviewCount,
      lapseCount: 0,
      ease: 2.5,
      repetition: Math.min(hint.reviewCount, 4),
      intervalDays: hint.reviewCount === 0 ? 0 : Math.max(1, Math.round(hint.reviewCount * 1.5)),
      dueAt: hint.dueAt,
      lastReviewedAt: hint.lastReviewedAt,
      updatedAt: hint.lastReviewedAt ?? hint.addedAt ?? fallbackUpdatedAt,
    };
  }

  return states;
}

const TRACK_OF_ACTIVITY: Record<string, TrainingTrack> = {
  vocabulary: "vocabulary",
  review: "vocabulary",
  grammar: "grammar",
  speaking: "speaking",
  assessment: "vocabulary",
  knowledge: "vocabulary",
};

/** 把第一阶段的学习记录转换为训练记录（示例历史） */
function buildSeedAttempts(): TrainingAttempt[] {
  return learningRecords.map((record, index) => {
    const track = TRACK_OF_ACTIVITY[record.activity] ?? "vocabulary";
    const kind =
      track === "speaking" ? "shadowing" : track === "grammar" ? "error-correction" : "multiple-choice";
    const answers: ExerciseAnswer[] = Array.from({ length: record.items }, (_, i) => ({
      exerciseId: `ex_seed_${index}_${i}`,
      value: "",
      correct: i < record.correct,
      partialScore: i < record.correct ? 1 : 0,
      elapsedMs: 0,
    }));

    return {
      id: `ta_seed_${index + 1}`,
      submissionId: `seed_${index + 1}`,
      sessionId: `sess_seed_${index + 1}`,
      kind,
      track,
      title: record.title,
      difficulty: 3,
      minutes: record.minutes,
      durationMs: record.minutes * 60000,
      score: record.correct,
      maxScore: record.items,
      accuracy: record.items === 0 ? 0 : Math.round((record.correct / record.items) * 100),
      xp: record.xp,
      perKind: [{ kind, correct: record.correct, total: record.items }],
      answers,
      entityIds: record.entityIds,
      startedAt: new Date(new Date(record.occurredAt).getTime() - record.minutes * 60000).toISOString(),
      finishedAt: record.occurredAt,
    };
  });
}

function buildSeedAssessments(): AssessmentAttempt[] {
  return assessmentModules.flatMap((module) => {
    if (!module.lastAttempt) return [];
    const perSection = module.sections.map((section) => ({
      sectionId: section.id,
      title: section.title,
      correct: 0,
      total: section.items.length,
      score: module.lastAttempt?.score ?? 0,
    }));

    return [
      {
        id: `aa_seed_${module.id}`,
        submissionId: `seed_${module.id}`,
        moduleId: module.id,
        score: module.lastAttempt.score,
        accuracy: module.lastAttempt.score,
        level: "B1",
        perSection,
        answers: [],
        strengths: [],
        weaknesses: [],
        recommendation: "",
        minutes: module.minutes,
        finishedAt: module.lastAttempt.at,
      } satisfies AssessmentAttempt,
    ];
  });
}

/**
 * 种子掌握度轨迹：把每个条目的当前掌握度按复习次数平摊为若干次提升，
 * 让趋势图有可回放的历史，而不是一条直线。
 */
function buildSeedMasteryEvents(states: Record<string, EntityState>): MasteryEvent[] {
  const events: MasteryEvent[] = [];

  for (const state of Object.values(states)) {
    if (state.reviewCount <= 0 || state.mastery <= 0) continue;
    const steps = Math.min(state.reviewCount, 4);
    const step = state.mastery / steps;
    const anchor = new Date(state.lastReviewedAt ?? state.updatedAt);

    for (let i = 0; i < steps; i += 1) {
      const at = addDays(anchor, -(steps - 1 - i) * 3);
      events.push({
        id: `me_seed_${state.entityId}_${i}`,
        entityId: state.entityId,
        kind: state.kind,
        before: Math.round(step * i * 10) / 10,
        after: Math.round(step * (i + 1) * 10) / 10,
        source: "training",
        refId: "seed",
        at: at.toISOString(),
      });
    }
  }

  return events;
}
