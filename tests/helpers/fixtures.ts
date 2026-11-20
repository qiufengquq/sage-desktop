import type {
  AssessmentAttempt,
  EntityState,
  ExerciseAnswer,
  ReviewLog,
  TrainingAttempt,
} from "@/types";

/** 固定的基准时间，避免测试依赖真实时间 */
export const NOW = new Date("2026-11-14T20:00:00+08:00");

export function entityState(overrides: Partial<EntityState> = {}): EntityState {
  return {
    entityId: "w_test",
    kind: "word",
    mastery: 0,
    strength: 0,
    state: "new",
    reviewCount: 0,
    lapseCount: 0,
    ease: 2.5,
    repetition: 0,
    intervalDays: 0,
    dueAt: null,
    lastReviewedAt: null,
    updatedAt: NOW.toISOString(),
    ...overrides,
  };
}

export function answer(overrides: Partial<ExerciseAnswer> = {}): ExerciseAnswer {
  return {
    exerciseId: "ex_1",
    value: "answer",
    correct: true,
    partialScore: 1,
    elapsedMs: 4200,
    ...overrides,
  };
}

export function trainingAttempt(overrides: Partial<TrainingAttempt> = {}): TrainingAttempt {
  return {
    id: "ta_1",
    submissionId: "sub_1",
    sessionId: "sess_1",
    kind: "multiple-choice",
    track: "vocabulary",
    title: "测试训练",
    difficulty: 3,
    minutes: 10,
    durationMs: 600000,
    score: 8,
    maxScore: 10,
    accuracy: 80,
    xp: 30,
    perKind: [{ kind: "multiple-choice", correct: 8, total: 10 }],
    answers: [answer(), answer({ exerciseId: "ex_2", correct: false, partialScore: 0 })],
    entityIds: ["w_test"],
    startedAt: new Date(NOW.getTime() - 600000).toISOString(),
    finishedAt: NOW.toISOString(),
    ...overrides,
  };
}

export function assessmentAttempt(overrides: Partial<AssessmentAttempt> = {}): AssessmentAttempt {
  return {
    id: "aa_1",
    submissionId: "asub_1",
    moduleId: "vocabulary",
    score: 70,
    accuracy: 70,
    level: "B1",
    perSection: [],
    answers: [],
    strengths: [],
    weaknesses: [],
    recommendation: "",
    minutes: 12,
    finishedAt: NOW.toISOString(),
    ...overrides,
  };
}

export function reviewLog(overrides: Partial<ReviewLog> = {}): ReviewLog {
  return {
    id: "rl_1",
    entityId: "w_test",
    rating: "good",
    prevDueAt: null,
    nextDueAt: new Date(NOW.getTime() + 86400000).toISOString(),
    prevIntervalDays: 0,
    nextIntervalDays: 1,
    ease: 2.5,
    masteryBefore: 40,
    masteryAfter: 55,
    xp: 6,
    at: NOW.toISOString(),
    ...overrides,
  };
}
