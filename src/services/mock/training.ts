import type {
  Exercise,
  ExerciseAnswer,
  ExerciseKind,
  ExerciseResult,
  KindScore,
  TrainingSession,
  TrainingTrack,
} from "@/types";
import { createId } from "@/lib/utils";
import { scoreAnswer } from "@/lib/grading";
import { exercisesOf } from "./exercises";

export function exercisesFor(kind: ExerciseKind, count?: number): Exercise[] {
  const pool = exercisesOf(kind);
  return typeof count === "number" ? pool.slice(0, count) : pool;
}

export function buildSession(
  kind: ExerciseKind,
  track: TrainingTrack,
  options: { count?: number; title: string; exercises?: Exercise[] } = { title: "" },
): TrainingSession {
  const exercises = options.exercises ?? exercisesFor(kind, options.count);
  return {
    id: createId("sess"),
    track,
    kind,
    title: options.title || exercises[0]?.prompt.instruction || "训练",
    exercises,
    startedAt: new Date().toISOString(),
    estimatedMinutes: Math.max(2, Math.round((exercises.length * 58) / 60)),
    targetEntityIds: Array.from(new Set(exercises.flatMap((e) => e.entityIds))),
  };
}

export function gradeSession(
  session: TrainingSession,
  answers: ExerciseAnswer[],
): ExerciseResult {
  const byId = new Map(answers.map((a) => [a.exerciseId, a]));
  const perKindMap = new Map<ExerciseKind, KindScore>();

  let total = 0;
  let earned = 0;
  let durationMs = 0;
  const weakEntityIds = new Set<string>();

  for (const exercise of session.exercises) {
    const answer = byId.get(exercise.id);
    const outcome = answer
      ? scoreAnswer(exercise, answer.value)
      : { correct: false, partial: 0 };

    total += 1;
    earned += outcome.partial;
    durationMs += answer?.elapsedMs ?? 0;

    const bucket = perKindMap.get(exercise.kind) ?? {
      kind: exercise.kind,
      correct: 0,
      total: 0,
    };
    bucket.total += 1;
    if (outcome.correct) bucket.correct += 1;
    perKindMap.set(exercise.kind, bucket);

    if (outcome.partial < 0.7) exercise.entityIds.forEach((id) => weakEntityIds.add(id));
  }

  const accuracy = total === 0 ? 0 : Math.round((earned / total) * 100);
  const xp = Math.round(
    session.exercises.reduce(
      (acc, e) => acc + e.xp * scoreAnswer(e, byId.get(e.id)?.value ?? "").partial,
      0,
    ),
  );

  return {
    sessionId: session.id,
    track: session.track,
    answers,
    score: Math.round(earned * 10) / 10,
    maxScore: total,
    accuracy,
    durationMs,
    xp,
    perKind: Array.from(perKindMap.values()),
    weakEntityIds: Array.from(weakEntityIds),
    finishedAt: new Date().toISOString(),
  };
}
