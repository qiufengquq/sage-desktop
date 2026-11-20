import type {
  AnswerCheck,
  ApiResult,
  ExerciseAnswer,
  ExerciseKind,
  ExerciseResult,
  RequestOptions,
  TrainingOption,
  TrainingSession,
  TrainingTrack,
} from "@/types";
import { latency } from "@/services/mock/base";
import { trainingOptions, exerciseById, exercisesOf } from "@/services/mock/exercises";
import { buildSession, gradeSession } from "@/services/mock/training";
import { applyTrainingSubmit } from "@/services/data/learning-db";
import { selectExercises } from "@/lib/algorithms/selection";
import { answerOf, scoreAnswer } from "@/lib/grading";
import { commitDb, getDb } from "@/stores/learning-store";
import { COMMANDS } from "./contracts";
import { serve } from "./transport";

/** 训练入口的题量必须与题库实际容量一致 */
function optionsFromDb(): TrainingOption[] {
  return trainingOptions.map((option) => {
    const available = exercisesOf(option.kind).length;
    return { ...option, itemCount: Math.max(1, Math.min(option.itemCount, available)) };
  });
}

export function getTrainingOptions(
  track?: TrainingTrack,
  options: RequestOptions = {},
): Promise<ApiResult<TrainingOption[]>> {
  return serve(
    COMMANDS.trainingOptions,
    async () => {
      await latency(options.signal);
      const list = optionsFromDb();
      return track ? list.filter((option) => option.track === track) : list;
    },
    { args: { track }, request: options },
  );
}

/**
 * 开始训练：
 * - 题量取训练入口声明的 itemCount（与实际题库容量取小）
 * - 难度取当前计划难度
 * - 近期做过的题降权，避免每次都是同一批
 */
export function startSession(input: {
  kind: ExerciseKind;
  track: TrainingTrack;
  count?: number;
  title?: string;
  difficulty?: number;
}): Promise<ApiResult<TrainingSession>> {
  return serve(
    COMMANDS.trainingStart,
    async () => {
      await latency();
      const db = getDb();
      const option = trainingOptions.find((item) => item.kind === input.kind);
      const count = input.count ?? option?.itemCount ?? 4;
      const targetDifficulty = input.difficulty ?? db.plan?.difficulty.current ?? 3;
      const seenAt = new Map<string, number>();
      for (const attempt of db.trainingAttempts) {
        const time = new Date(attempt.finishedAt).getTime();
        for (const answer of attempt.answers) {
          const previous = seenAt.get(answer.exerciseId) ?? 0;
          if (time > previous) seenAt.set(answer.exerciseId, time);
        }
      }

      const exercises = selectExercises({
        pool: exercisesOf(input.kind),
        count,
        targetDifficulty,
        seenAt,
        now: new Date(),
      });

      return buildSession(input.kind, input.track, {
        count,
        title: input.title ?? "",
        exercises,
      });
    },
    { args: { kind: input.kind, track: input.track, count: input.count } },
  );
}

/**
 * 单题即时校验（判分只在服务端）。
 *
 * backend 模式：`training_check` 会用会话快照里的题目判分并回传可读答案；
 * mock 模式：用本地题库的同一套规则判分，产物同形，页面无需分支。
 */
export function checkAnswer(input: {
  sessionId: string;
  exerciseId: string;
  value: ExerciseAnswer["value"];
}): Promise<ApiResult<AnswerCheck>> {
  return serve(
    COMMANDS.trainingCheck,
    async () => {
      const exercise = exerciseById(input.exerciseId);
      if (!exercise) {
        throw new Error(`题目不存在：${input.exerciseId}`);
      }
      const outcome = scoreAnswer(exercise, input.value);
      return {
        exerciseId: input.exerciseId,
        correct: outcome.correct,
        partialScore: outcome.partial,
        correctAnswer: answerOf(exercise),
        explanation: exercise.explanation,
      } satisfies AnswerCheck;
    },
    { args: { sessionId: input.sessionId, exerciseId: input.exerciseId, value: input.value } },
  );
}

/** 提交训练：判分 + 写回记录/掌握度/复习队列/XP/计划完成态 */
export function submitSession(input: {
  session: TrainingSession;
  answers: ExerciseAnswer[];
}): Promise<ApiResult<ExerciseResult>> {
  return serve(
    COMMANDS.trainingSubmit,
    async () => {
      await latency();
      const result = gradeSession(input.session, input.answers);
      const now = new Date();
      let duplicate = false;
      commitDb((db) => {
        const applied = applyTrainingSubmit(db, {
          session: input.session,
          exercises: input.session.exercises,
          answers: input.answers,
          result,
          now,
        });
        duplicate = applied.duplicate;
        return applied.db;
      });
      // 幂等语义与后端一致：重复提交返回既有结果并标记 duplicate
      return { ...result, duplicate };
    },
    { args: { sessionId: input.session.id, answers: input.answers } },
  );
}
