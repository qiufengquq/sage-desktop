import type {
  AbilityProfile,
  AssessmentAttempt,
  Difficulty,
  EntityKind,
  EntityState,
  Exercise,
  ExerciseAnswer,
  ExerciseResult,
  LearningDb,
  MasteryEvent,
  PlanItem,
  PlanItemAction,
  ReviewLog,
  ReviewRating,
  TrainingAttempt,
  TrainingSession,
} from "@/types";
import { buildAbilityProfile } from "@/lib/algorithms/ability";
import { updateMastery } from "@/lib/algorithms/mastery";
import { generatePlan } from "@/lib/algorithms/planner";
import { isDue, scheduleNext } from "@/lib/algorithms/srs";
import { buildDailyStats, weeklyFrom } from "@/lib/algorithms/stats";
import { xpForReview } from "@/lib/algorithms/xp";
import { offsetMinutesForTimeZone } from "@/lib/date";

/**
 * 领域写入层：所有"提交一次业务动作"都落在这里。
 *
 * 一致性约定：
 * - 每个函数接收完整 db，返回新的 db（不可变更新），调用方通过 store.commit 原子替换。
 * - 事件带 submissionId，重复提交不会重复计分（幂等）。
 * - 掌握度、复习调度、XP 都委托给 lib/algorithms 的纯函数，便于两端对照实现。
 */

export function kindOfEntityId(entityId: string): EntityKind {
  if (entityId.startsWith("g_")) return "grammar";
  if (entityId.startsWith("s_")) return "sentence";
  if (entityId.startsWith("ph_")) return "phrase";
  return "word";
}

function emptyState(entityId: string, now: Date): EntityState {
  return {
    entityId,
    kind: kindOfEntityId(entityId),
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
    updatedAt: now.toISOString(),
  };
}

/** 训练正确率 → 复习自评，让训练结果立即影响复习计划 */
export function ratingForPartial(partial: number): ReviewRating {
  if (partial >= 0.9) return "good";
  if (partial >= 0.7) return "hard";
  return "again";
}

interface StateUpdateResult {
  state: EntityState;
  event: MasteryEvent;
}

/** 单实体掌握度 + 复习调度的一次更新 */
function applyEntityPerformance(input: {
  state: EntityState;
  partial: number;
  difficulty: Difficulty;
  source: "training" | "review" | "assessment";
  refId: string;
  now: Date;
}): StateUpdateResult {
  const { state, partial, difficulty, source, refId, now } = input;
  const update = updateMastery({
    snapshot: state,
    partialScore: partial,
    difficulty,
    now,
  });
  const schedule = scheduleNext(
    {
      ease: state.ease,
      intervalDays: state.intervalDays,
      repetition: state.repetition,
      lapseCount: state.lapseCount,
    },
    ratingForPartial(partial),
    now,
  );

  return {
    state: {
      ...state,
      mastery: update.next,
      strength: update.strength,
      state: update.state,
      reviewCount: update.reviewCount,
      lapseCount: schedule.lapseCount,
      ease: schedule.ease,
      repetition: schedule.repetition,
      intervalDays: schedule.intervalDays,
      dueAt: schedule.dueAt,
      lastReviewedAt: now.toISOString(),
      updatedAt: now.toISOString(),
    },
    event: {
      id: `me_${now.getTime()}_${state.entityId}_${source}`,
      entityId: state.entityId,
      kind: state.kind,
      before: update.previous,
      after: update.next,
      source,
      refId,
      at: now.toISOString(),
    },
  };
}

function exerciseAnswerMap(answers: ExerciseAnswer[]): Map<string, ExerciseAnswer> {
  return new Map(answers.map((answer) => [answer.exerciseId, answer]));
}

export interface TrainingSubmitInput {
  session: TrainingSession;
  exercises: Exercise[];
  answers: ExerciseAnswer[];
  result: ExerciseResult;
  now: Date;
}

export interface SubmitOutcome {
  db: LearningDb;
  duplicate: boolean;
  attempt: TrainingAttempt;
}

/** 提交训练：写记录 + 更新掌握度 + 入复习队列 + 累计 XP + 标记计划项 */
export function applyTrainingSubmit(db: LearningDb, input: TrainingSubmitInput): SubmitOutcome {
  const submissionId = input.session.id;
  const existing = db.trainingAttempts.find((attempt) => attempt.submissionId === submissionId);

  if (existing) {
    return { db, duplicate: true, attempt: existing };
  }

  const { session, exercises, answers, result, now } = input;
  const answerById = exerciseAnswerMap(answers);
  const minutes = Math.max(1, Math.round(result.durationMs / 60000));
  const difficulty = (exercises[0]?.difficulty ?? 3) as Difficulty;

  const attempt: TrainingAttempt = {
    id: `ta_${now.getTime()}`,
    submissionId,
    sessionId: session.id,
    kind: session.kind,
    track: session.track,
    title: session.title,
    difficulty,
    minutes,
    durationMs: result.durationMs,
    score: result.score,
    maxScore: result.maxScore,
    accuracy: result.accuracy,
    xp: result.xp,
    perKind: result.perKind,
    answers,
    entityIds: Array.from(new Set(exercises.flatMap((exercise) => exercise.entityIds))),
    startedAt: session.startedAt,
    finishedAt: now.toISOString(),
  };

  const states = { ...db.entityStates };
  const events: MasteryEvent[] = [];

  for (const exercise of exercises) {
    const answer = answerById.get(exercise.id);
    const partial = answer?.partialScore ?? 0;
    for (const entityId of exercise.entityIds) {
      const state = states[entityId] ?? emptyState(entityId, now);
      const update = applyEntityPerformance({
        state,
        partial,
        difficulty: exercise.difficulty,
        source: "training",
        refId: attempt.id,
        now,
      });
      states[entityId] = update.state;
      events.push(update.event);
    }
  }

  const plan = markPlanItemDone(db.plan, {
    type: "training",
    kind: session.kind,
    track: session.track,
  });

  return {
    db: {
      ...db,
      entityStates: states,
      trainingAttempts: [...db.trainingAttempts, attempt],
      masteryEvents: [...db.masteryEvents, ...events],
      plan,
    },
    duplicate: false,
    attempt,
  };
}

export interface AssessmentSubmitInput {
  attempt: AssessmentAttempt;
  exercises: Exercise[];
  answers: ExerciseAnswer[];
  now: Date;
}

/** 提交测评：写记录 + 更新涉及实体的掌握度 */
export function applyAssessmentSubmit(
  db: LearningDb,
  input: AssessmentSubmitInput,
): { db: LearningDb; duplicate: boolean } {
  const existing = db.assessmentAttempts.find(
    (attempt) => attempt.submissionId === input.attempt.submissionId,
  );
  if (existing) return { db, duplicate: true };

  const answerById = exerciseAnswerMap(input.answers);
  const states = { ...db.entityStates };
  const events: MasteryEvent[] = [];

  for (const exercise of input.exercises) {
    const answer = answerById.get(exercise.id);
    const partial = answer?.partialScore ?? 0;
    for (const entityId of exercise.entityIds) {
      const state = states[entityId] ?? emptyState(entityId, input.now);
      const update = applyEntityPerformance({
        state,
        partial,
        difficulty: exercise.difficulty,
        // 测评产生的掌握度变化必须可区分，否则趋势回放无法归因
        source: "assessment",
        refId: input.attempt.id,
        now: input.now,
      });
      states[entityId] = update.state;
      events.push(update.event);
    }
  }

  return {
    db: {
      ...db,
      entityStates: states,
      assessmentAttempts: [...db.assessmentAttempts, input.attempt],
      masteryEvents: [...db.masteryEvents, ...events],
    },
    duplicate: false,
  };
}

export interface ReviewSubmitInput {
  entityId: string;
  rating: ReviewRating;
  difficulty: Difficulty;
  now: Date;
}

/** 提交一次复习：更新掌握度 + 重新排期 + 记录日志与 XP */
export function applyReviewSubmit(
  db: LearningDb,
  input: ReviewSubmitInput,
): { db: LearningDb; log: ReviewLog; state: EntityState } {
  const { entityId, rating, difficulty, now } = input;
  const state = db.entityStates[entityId] ?? emptyState(entityId, now);
  const partialOf: Record<ReviewRating, number> = { again: 0, hard: 0.55, good: 0.9, easy: 1 };
  const partial = partialOf[rating];

  const update = updateMastery({ snapshot: state, partialScore: partial, difficulty, now });
  const schedule = scheduleNext(
    {
      ease: state.ease,
      intervalDays: state.intervalDays,
      repetition: state.repetition,
      lapseCount: state.lapseCount,
    },
    rating,
    now,
  );

  const nextState: EntityState = {
    ...state,
    mastery: update.next,
    strength: update.strength,
    state: update.state,
    reviewCount: update.reviewCount,
    lapseCount: schedule.lapseCount,
    ease: schedule.ease,
    repetition: schedule.repetition,
    intervalDays: schedule.intervalDays,
    dueAt: schedule.dueAt,
    lastReviewedAt: now.toISOString(),
    updatedAt: now.toISOString(),
  };

  const log: ReviewLog = {
    id: `rl_${now.getTime()}_${entityId}`,
    entityId,
    rating,
    prevDueAt: state.dueAt,
    nextDueAt: schedule.dueAt,
    prevIntervalDays: state.intervalDays,
    nextIntervalDays: schedule.intervalDays,
    ease: schedule.ease,
    masteryBefore: update.previous,
    masteryAfter: update.next,
    xp: xpForReview(rating),
    at: now.toISOString(),
  };

  const event: MasteryEvent = {
    id: `me_${now.getTime()}_${entityId}_review`,
    entityId,
    kind: state.kind,
    before: update.previous,
    after: update.next,
    source: "review",
    refId: log.id,
    at: now.toISOString(),
  };

  return {
    db: {
      ...db,
      entityStates: { ...db.entityStates, [entityId]: nextState },
      reviewLogs: [...db.reviewLogs, log],
      masteryEvents: [...db.masteryEvents, event],
      plan: markPlanItemDone(db.plan, { type: "review" }),
    },
    log,
    state: nextState,
  };
}

function markPlanItemDone(
  plan: LearningDb["plan"],
  action: PlanItemAction,
): LearningDb["plan"] {
  if (!plan) return plan;
  const matches = (item: PlanItem) => {
    if (item.done || item.action.type !== action.type) return false;
    if (item.action.type === "training" && action.type === "training") {
      return item.action.kind === action.kind;
    }
    return true;
  };
  if (!plan.today.items.some(matches)) return plan;

  const items = plan.today.items.map((item) =>
    matches(item) ? { ...item, done: true, status: "done" as const } : item,
  );
  const done = items.filter((item) => item.done).length;

  return {
    ...plan,
    today: { ...plan.today, items, completed: done === items.length },
    completion: {
      done,
      total: items.length,
      rate: Math.round((done / Math.max(1, items.length)) * 100),
    },
  };
}

export function applyPlanToggle(db: LearningDb, itemId: string): LearningDb {
  if (!db.plan) return db;
  const items = db.plan.today.items.map((item) =>
    item.id === itemId
      ? { ...item, done: !item.done, status: !item.done ? ("done" as const) : ("pending" as const) }
      : item,
  );
  const done = items.filter((item) => item.done).length;

  return {
    ...db,
    plan: {
      ...db.plan,
      today: { ...db.plan.today, items, completed: done === items.length },
      completion: {
        done,
        total: items.length,
        rate: Math.round((done / Math.max(1, items.length)) * 100),
      },
    },
  };
}

export function applyDifficulty(db: LearningDb, level: number): LearningDb {
  const clamped = Math.min(5, Math.max(1, Math.round(level * 10) / 10));
  const plan = db.plan;
  const nextPlan = plan
    ? {
        ...plan,
        difficulty: {
          ...plan.difficulty,
          current: clamped,
          requiresConfirmation: false,
        },
      }
    : plan;

  return {
    ...db,
    plan: nextPlan,
    planMeta: { ...db.planMeta, difficultyOverride: clamped },
  };
}

export function countDue(db: LearningDb, now: Date): number {
  return Object.values(db.entityStates).filter((state) => isDue(state.dueAt, now)).length;
}

export function currentAbility(db: LearningDb, now: Date): AbilityProfile {
  return buildAbilityProfile({
    profile: db.profile,
    assessmentAttempts: db.assessmentAttempts,
    trainingAttempts: db.trainingAttempts,
    states: db.entityStates,
    now,
  });
}

/**
 * 重新生成计划。
 * 输入全部来自 db：画像、到期数量、最近 10 次正确率、本周统计、每日时长。
 */
export function rebuildPlan(
  db: LearningDb,
  options: { reason: "regenerate" | "assessment" | "scheduled"; now: Date },
): LearningDb {
  const { now } = options;
  const offsetMinutes = offsetMinutesForTimeZone(db.profile.timezone, now);
  const ability = currentAbility(db, now);
  const daily = buildDailyStats({
    attempts: db.trainingAttempts,
    reviewLogs: db.reviewLogs,
    days: 30,
    now,
    offsetMinutes,
  });
  const weeks = weeklyFrom(daily, now, offsetMinutes);
  const thisWeek = weeks[weeks.length - 1];
  const recent = [...db.trainingAttempts]
    .sort((a, b) => new Date(b.finishedAt).getTime() - new Date(a.finishedAt).getTime())
    .slice(0, 10);
  const recentAccuracy =
    recent.length === 0
      ? null
      : Math.round(recent.reduce((acc, attempt) => acc + attempt.accuracy, 0) / recent.length);

  const weekStart = new Date(now);
  weekStart.setDate(weekStart.getDate() - 7);
  const weekAttempts = db.trainingAttempts.filter(
    (attempt) => new Date(attempt.finishedAt).getTime() >= weekStart.getTime(),
  );
  const weeklyGrammarCorrect = weekAttempts
    .filter((attempt) => attempt.track === "grammar")
    .reduce(
      (acc, attempt) =>
        acc + attempt.perKind.reduce((sum, bucket) => sum + bucket.correct, 0),
      0,
    );
  const weeklyNewMastered = Object.values(db.entityStates).filter(
    (state) =>
      state.state === "mastered" &&
      state.lastReviewedAt !== null &&
      new Date(state.lastReviewedAt).getTime() >= weekStart.getTime(),
  ).length;

  const plan = generatePlan({
    profile: db.profile,
    ability,
    states: db.entityStates,
    dueCount: countDue(db, now),
    recentAccuracy,
    weeklyMinutes: thisWeek?.minutes ?? 0,
    weeklyActiveDays: thisWeek?.activeDays ?? 0,
    weeklyNewMastered,
    weeklyGrammarCorrect,
    difficultyOverride: db.planMeta.difficultyOverride,
    generatedBy: options.reason,
    now,
    previous: db.plan,
  });

  return {
    ...db,
    plan,
    planMeta: {
      generatedBy: options.reason,
      generatedAt: now.toISOString(),
      difficultyOverride: db.planMeta.difficultyOverride,
    },
  };
}

/** 清理学习记录：保留知识库内容与个人资料，清空事件与学习态 */
export function clearLearningRecords(db: LearningDb, now: Date): LearningDb {
  const states: Record<string, EntityState> = {};
  for (const [id, state] of Object.entries(db.entityStates)) {
    states[id] = {
      ...emptyState(id, now),
      kind: state.kind,
      ease: 2.5,
      updatedAt: now.toISOString(),
    };
  }

  return {
    ...db,
    entityStates: states,
    trainingAttempts: [],
    assessmentAttempts: [],
    reviewLogs: [],
    masteryEvents: [],
    plan: null,
    planMeta: { generatedBy: "seed", generatedAt: now.toISOString(), difficultyOverride: null },
  };
}
