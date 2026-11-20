import type {
  AnalyticsData,
  DashboardData,
  DashboardSnapshot,
  Difficulty,
  EntityState,
  LearningDb,
  LearningRecord,
  LocalDataSummary,
  MasteryState,
  ReviewCard,
  ReviewRating,
  ReviewRecord,
  StudyPlan,
  TrainingAttempt,
} from "@/types";
import { PART_OF_SPEECH_LABEL } from "@/types/vocabulary";
import { buildAbilityProfile } from "@/lib/algorithms/ability";
import { isDue, overdueDaysOf, scheduleNext, SRS_PARAMS } from "@/lib/algorithms/srs";
import {
  averageMastery,
  buildDailyStats,
  computeStreak,
  forgettingCurveOf,
  masteryBreakdown,
  masteryTrendOf,
  timeBuckets,
  totalsFrom,
  weeklyFrom,
} from "@/lib/algorithms/stats";
import { addDays, offsetMinutesForTimeZone } from "@/lib/date";
import { trainingOptions } from "@/services/mock/exercises";
import { findGrammar } from "@/services/mock/grammar";
import { findSentence } from "@/services/mock/sentences";
import { findWord } from "@/services/mock/vocabulary";

/**
 * 领域读取层：把整份 db 派生成页面需要的 DTO。
 * 页面拿到的仍然是通过 services/api 返回的数据结构（与第一阶段一致），
 * 但内容改为真实计算，不再有硬编码。
 */

interface EntityLabel {
  label: string;
  secondary: string;
  kind: "word" | "grammar" | "sentence";
}

export function entityLabelOf(entityId: string): EntityLabel | null {
  const word = findWord(entityId);
  if (word) {
    return {
      label: word.term,
      secondary: word.meanings[0]?.definition ?? "",
      kind: "word",
    };
  }
  const grammar = findGrammar(entityId);
  if (grammar) {
    return { label: grammar.title, secondary: grammar.titleEn, kind: "grammar" };
  }
  const sentence = findSentence(entityId);
  if (sentence) {
    return { label: sentence.en, secondary: sentence.zh, kind: "sentence" };
  }
  return null;
}

function toReviewRecord(state: EntityState, now: Date): ReviewRecord | null {
  const label = entityLabelOf(state.entityId);
  if (!label || !state.dueAt) return null;
  return {
    id: `rv_${state.entityId}`,
    entityId: state.entityId,
    entityLabel: label.label,
    kind: label.kind,
    dueAt: state.dueAt,
    overdueDays: overdueDaysOf(state.dueAt, now),
    strength: state.strength,
    state: state.state,
    lastReviewedAt: state.lastReviewedAt,
  };
}

export function dueItems(db: LearningDb, now: Date): EntityState[] {
  return Object.values(db.entityStates)
    .filter((state) => isDue(state.dueAt, now))
    .sort((a, b) => {
      const aDue = a.dueAt ? new Date(a.dueAt).getTime() : 0;
      const bDue = b.dueAt ? new Date(b.dueAt).getTime() : 0;
      return aDue - bDue;
    });
}

export function reviewQueueOf(db: LearningDb, now: Date, limit?: number): ReviewRecord[] {
  const records = dueItems(db, now)
    .map((state) => toReviewRecord(state, now))
    .filter((record): record is ReviewRecord => record !== null);
  return limit ? records.slice(0, limit) : records;
}

function toLearningRecord(attempt: TrainingAttempt): LearningRecord {
  return {
    id: attempt.id,
    activity:
      attempt.track === "speaking"
        ? "speaking"
        : attempt.track === "grammar"
          ? "grammar"
          : "vocabulary",
    title: attempt.title,
    minutes: attempt.minutes,
    items: attempt.answers.length,
    correct: attempt.answers.filter((answer) => answer.correct).length,
    xp: attempt.xp,
    occurredAt: attempt.finishedAt,
    entityIds: attempt.entityIds,
  };
}

const ASSESSMENT_TITLE: Record<string, string> = {
  vocabulary: "词汇",
  grammar: "语法",
  speaking: "口语",
};

function assessmentTitle(moduleId: string): string {
  return ASSESSMENT_TITLE[moduleId] ?? "综合";
}

function accuracyOfTrack(attempts: TrainingAttempt[], track: TrainingAttempt["track"]): number {
  const relevant = attempts.filter((attempt) => attempt.track === track);
  const items = relevant.reduce((acc, attempt) => acc + attempt.answers.length, 0);
  const correct = relevant.reduce(
    (acc, attempt) => acc + attempt.answers.filter((answer) => answer.correct).length,
    0,
  );
  return items === 0 ? 0 : Math.round((correct / items) * 100);
}

export function buildDashboardData(db: LearningDb, now: Date): DashboardData {
  // 归日与"当前时刻"一律按用户配置的时区换算，避免跨时区统计漂移
  const offsetMinutes = offsetMinutesForTimeZone(db.profile.timezone, now);
  const daily = buildDailyStats({
    attempts: db.trainingAttempts,
    reviewLogs: db.reviewLogs,
    days: 30,
    now,
    offsetMinutes,
  });
  const today = daily[daily.length - 1] ?? {
    date: "",
    minutes: 0,
    items: 0,
    correct: 0,
    xp: 0,
    vocabularyReviewed: 0,
    grammarReviewed: 0,
  };
  const streak = computeStreak(daily);
  const states = Object.values(db.entityStates);
  const wordStates = states.filter((s) => s.kind === "word" || s.kind === "phrase");
  const grammarStates = states.filter((s) => s.kind === "grammar");

  const snapshot: DashboardSnapshot = {
    today: {
      minutes: today.minutes,
      goalMinutes: db.profile.dailyMinutes,
      items: today.items,
      accuracy: today.items === 0 ? 0 : Math.round((today.correct / today.items) * 100),
      xp: today.xp,
    },
    streak,
    totals: {
      minutes: daily.reduce((acc, day) => acc + day.minutes, 0),
      words: wordStates.length,
      masteredWords: wordStates.filter((s) => s.state === "mastered").length,
      grammarPoints: grammarStates.length,
      masteredGrammar: grammarStates.filter((s) => s.state === "mastered").length,
      days: daily.filter((day) => day.minutes > 0).length,
    },
    mastery: {
      vocabulary: masteryBreakdown(wordStates),
      grammar: masteryBreakdown(grammarStates),
    },
    vocabularyAccuracy: accuracyOfTrack(db.trainingAttempts, "vocabulary"),
    grammarAccuracy: accuracyOfTrack(db.trainingAttempts, "grammar"),
  };

  const plan = db.plan;
  const tasks = (plan?.today.items ?? []).map((item) => ({
    id: item.id,
    title: item.title,
    activity: item.activity,
    minutes: item.minutes,
    xp: item.xp,
    reason: item.reason,
    status: item.status,
    action: item.action,
  }));

  const shiftedNow = new Date(now.getTime() + offsetMinutes * 60_000);
  const minutesNow = shiftedNow.getUTCHours() * 60 + shiftedNow.getUTCMinutes();

  return {
    snapshot,
    tasks,
    reviewTasks: reviewQueueOf(db, now, 5),
    recentRecords: [
      ...db.trainingAttempts.map(toLearningRecord),
      ...db.assessmentAttempts.map((attempt) => ({
        id: attempt.id,
        activity: "assessment" as const,
        title: `${assessmentTitle(attempt.moduleId)} 测评`,
        minutes: attempt.minutes,
        items: attempt.answers.length,
        correct: attempt.answers.filter((answer) => answer.correct).length,
        xp: 0,
        occurredAt: attempt.finishedAt,
        entityIds: [],
      })),
    ]
      .sort((a, b) => new Date(b.occurredAt).getTime() - new Date(a.occurredAt).getTime())
      .slice(0, 5),
    recommendation: plan?.advice[0] ?? null,
    quickStart: trainingOptions.slice(0, 3),
    planCompletion: plan?.completion.rate ?? 0,
    nowProgress: Math.min(1, minutesNow / (24 * 60)),
  };
}

export function buildAnalyticsData(db: LearningDb, range: 7 | 14 | 30, now: Date): AnalyticsData {
  const offsetMinutes = offsetMinutesForTimeZone(db.profile.timezone, now);
  const daily = buildDailyStats({
    attempts: db.trainingAttempts,
    reviewLogs: db.reviewLogs,
    days: range,
    now,
    offsetMinutes,
  });
  const since = addDays(now, -range).getTime();
  const attemptsInRange = db.trainingAttempts.filter(
    (attempt) => new Date(attempt.finishedAt).getTime() >= since,
  );
  const reviewsInRange = db.reviewLogs.filter((log) => new Date(log.at).getTime() >= since);
  const states = Object.values(db.entityStates);

  const trends = (["vocabulary", "grammar", "speaking"] as const).map((kind) =>
    masteryTrendOf({
      kind,
      states: db.entityStates,
      events: db.masteryEvents,
      attempts: db.trainingAttempts,
      days: range,
      now,
      offsetMinutes,
    }),
  );

  return {
    range,
    daily,
    weekly: weeklyFrom(daily, now, offsetMinutes),
    trends,
    forgettingCurve: forgettingCurveOf(states),
    timeDistribution: timeBuckets(attemptsInRange, reviewsInRange, offsetMinutes),
    reviewQueue: reviewQueueOf(db, now),
    mastered: states
      .filter((state) => state.state === "mastered")
      .map((state) => toReviewRecord(state, now))
      .filter((record): record is ReviewRecord => record !== null),
    totals: totalsFrom(daily),
    streak: computeStreak(daily),
  };
}

/** 计划里的能力画像会被重新计算，避免显示旧值 */
export function decoratePlan(db: LearningDb, now: Date): StudyPlan | null {
  if (!db.plan) return null;
  return {
    ...db.plan,
    profile: buildAbilityProfile({
      profile: db.profile,
      assessmentAttempts: db.assessmentAttempts,
      trainingAttempts: db.trainingAttempts,
      states: db.entityStates,
      now,
    }),
  };
}

export function localDataSummaryOf(db: LearningDb): LocalDataSummary {
  return {
    words: Object.values(db.entityStates).filter((s) => s.kind === "word" || s.kind === "phrase")
      .length,
    grammarPoints: Object.values(db.entityStates).filter((s) => s.kind === "grammar").length,
    sentences: Object.values(db.entityStates).filter((s) => s.kind === "sentence").length,
    records: db.trainingAttempts.length + db.assessmentAttempts.length + db.reviewLogs.length,
    sizeKb: Math.max(1, Math.round(JSON.stringify(db).length / 1024)),
  };
}

export function masteryOfKind(db: LearningDb, kinds: EntityState["kind"][]): number {
  return averageMastery(Object.values(db.entityStates).filter((s) => kinds.includes(s.kind)));
}

/**
 * 复习卡片：内容来自只读词库，调度预览来自 SM-2 状态。
 * 用户在看到答案前先回忆，揭晓后再自评，符合"检索练习"的基本流程。
 */
export function buildReviewCards(db: LearningDb, now: Date, limit = 20): ReviewCard[] {
  return dueItems(db, now)
    .slice(0, limit)
    .map((state) => {
      const content = reviewContentOf(state.entityId);
      if (!content) return null;

      const preview = {} as Record<ReviewRating, string>;
      for (const rating of ["again", "hard", "good", "easy"] as ReviewRating[]) {
        const outcome = scheduleNext(
          {
            ease: state.ease,
            intervalDays: state.intervalDays,
            repetition: state.repetition,
            lapseCount: state.lapseCount,
          },
          rating,
          now,
        );
        preview[rating] =
          outcome.intervalDays === 0
            ? `${SRS_PARAMS.relearnMinutes} 分钟后`
            : `${outcome.intervalDays} 天后`;
      }

      return {
        item: {
          entityId: state.entityId,
          kind: state.kind,
          label: content.label,
          secondary: content.secondary,
          difficulty: content.difficulty,
          mastery: state.mastery,
          strength: state.strength,
          state: state.state,
          dueAt: state.dueAt ?? now.toISOString(),
          overdueDays: state.dueAt ? overdueDaysOf(state.dueAt, now) : 0,
          lastReviewedAt: state.lastReviewedAt,
          reviewCount: state.reviewCount,
        },
        prompt: content.prompt,
        answer: content.answer,
        detail: content.detail,
        focus: `当前掌握度 ${Math.round(state.mastery)} 分 · 已复习 ${state.reviewCount} 次`,
        preview,
      } satisfies ReviewCard;
    })
    .filter((card): card is ReviewCard => card !== null);
}


export function stateOf(db: LearningDb, entityId: string): MasteryState | null {
  return db.entityStates[entityId]?.state ?? null;
}

interface ReviewContent {
  label: string;
  secondary: string;
  difficulty: Difficulty;
  prompt: string;
  answer: string;
  detail: string[];
}

/** 实体难度，复习时用于掌握度计算的加权 */
export function difficultyOf(entityId: string): Difficulty {
  return reviewContentOf(entityId)?.difficulty ?? 3;
}

/** 复习卡内容：不同实体类型给出不同的"回忆线索" */
function reviewContentOf(entityId: string): ReviewContent | null {
  const word = findWord(entityId);
  if (word) {
    return {
      label: word.term,
      secondary: `${word.phonetic} · ${word.meanings[0]?.definition ?? ""}`,
      difficulty: word.difficulty,
      prompt: word.term,
      answer: word.meanings
        .map((meaning) => `${PART_OF_SPEECH_LABEL[meaning.pos]} ${meaning.definition}`)
        .join("；"),
      detail: [
        ...word.examples.slice(0, 2).map((example) => `${example.en} —— ${example.zh}`),
        word.contexts.length > 0 ? `使用场景：${word.contexts.join(" / ")}` : "",
      ].filter(Boolean),
    };
  }

  const grammar = findGrammar(entityId);
  if (grammar) {
    return {
      label: grammar.title,
      secondary: grammar.titleEn,
      difficulty: grammar.difficulty,
      prompt: `${grammar.title}的核心规则是什么？`,
      answer: `${grammar.summary}\n结构：${grammar.structure}`,
      detail: grammar.examples.slice(0, 2).map((example) => `${example.en} —— ${example.zh}`),
    };
  }

  const sentence = findSentence(entityId);
  if (sentence) {
    const grammarTitles = sentence.grammarIds
      .map((id) => findGrammar(id)?.title ?? "")
      .filter(Boolean);
    return {
      label: sentence.en,
      secondary: sentence.zh,
      difficulty: sentence.difficulty,
      prompt: sentence.zh,
      answer: sentence.en,
      detail: grammarTitles.length > 0 ? [`涉及语法：${grammarTitles.join(" / ")}`] : [],
    };
  }

  return null;
}
