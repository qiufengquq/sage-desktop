import { beforeEach, describe, expect, it } from "vitest";
import type { ChoiceExercise, Exercise, ExerciseAnswer } from "@/types";
import { createMemoryRepository } from "@/services/data/repository";
import { setLearningRepository, getDb, resetLearningStore } from "@/stores/learning-store";
import { trainingService } from "@/services/api";
import { buildDashboardData } from "@/services/data/selectors";
import { dueItems } from "@/services/data/selectors";

function answersFor(exercises: Exercise[]): ExerciseAnswer[] {
  return exercises.map((exercise) => {
    const value =
      exercise.kind === "multiple-choice" || exercise.kind === "cloze"
        ? ((exercise as ChoiceExercise).options.find((option) => option.correct)?.id ?? "")
        : exercise.kind === "spelling" || exercise.kind === "dictation"
          ? exercise.answer
          : exercise.id;
    return {
      exerciseId: exercise.id,
      value,
      correct: true,
      partialScore: 1,
      elapsedMs: 15000,
    };
  });
}

describe("训练闭环（集成）", () => {
  beforeEach(() => {
    setLearningRepository(createMemoryRepository());
    resetLearningStore(new Date());
  });

  it("完成训练后：记录入库、掌握度上升、进入复习队列、XP 增加、首页同步", async () => {
    const beforeDb = getDb();
    const beforeDashboard = buildDashboardData(beforeDb, new Date());
    const attemptsBefore = beforeDb.trainingAttempts.length;

    const started = await trainingService.startSession({
      kind: "multiple-choice",
      track: "vocabulary",
      count: 2,
    });
    expect(started.ok).toBe(true);
    if (!started.ok) return;

    expect(started.data.exercises).toHaveLength(2);
    const entityId = started.data.exercises[0]?.entityIds[0] as string;
    expect(entityId).toBeTruthy();

    const masteryBefore = beforeDb.entityStates[entityId]?.mastery ?? 0;
    const answers = answersFor(started.data.exercises);

    const submitted = await trainingService.submitSession({ session: started.data, answers });
    expect(submitted.ok).toBe(true);
    if (!submitted.ok) return;

    const afterDb = getDb();
    expect(afterDb.trainingAttempts).toHaveLength(attemptsBefore + 1);

    const attempt = afterDb.trainingAttempts[afterDb.trainingAttempts.length - 1];
    expect(attempt?.answers).toHaveLength(2);
    expect(attempt?.accuracy).toBe(100);
    expect(attempt?.xp).toBeGreaterThan(0);

    expect(afterDb.entityStates[entityId]?.mastery).toBeGreaterThan(masteryBefore);
    expect(afterDb.entityStates[entityId]?.dueAt).not.toBeNull();
    expect(afterDb.masteryEvents.length).toBeGreaterThan(beforeDb.masteryEvents.length);

    const dueIds = dueItems(afterDb, new Date(Date.now() + 1000 * 60 * 60 * 24 * 40)).map(
      (state) => state.entityId,
    );
    expect(dueIds).toContain(entityId);

    const afterDashboard = buildDashboardData(afterDb, new Date());
    expect(afterDashboard.snapshot.today.xp).toBeGreaterThan(beforeDashboard.snapshot.today.xp);
    expect(afterDashboard.recentRecords[0]?.id).toBe(attempt?.id);
  });

  it("重复提交同一会话不会重复计分（幂等）", async () => {
    const started = await trainingService.startSession({
      kind: "multiple-choice",
      track: "vocabulary",
      count: 1,
    });
    if (!started.ok) throw new Error("session failed");

    const answers = answersFor(started.data.exercises);
    await trainingService.submitSession({ session: started.data, answers });
    const afterFirst = getDb().trainingAttempts.length;

    await trainingService.submitSession({ session: started.data, answers });
    expect(getDb().trainingAttempts.length).toBe(afterFirst);
  });

  it("训练会标记当天对应计划项完成", async () => {
    const plan = getDb().plan;
    const target = plan?.today.items.find((item) => item.action.type === "training");
    if (!target || target.action.type !== "training") return;

    const started = await trainingService.startSession({
      kind: target.action.kind,
      track: target.action.track,
      count: 1,
    });
    if (!started.ok) throw new Error("session failed");

    await trainingService.submitSession({
      session: started.data,
      answers: answersFor(started.data.exercises),
    });

    const updated = getDb().plan?.today.items.find((item) => item.id === target.id);
    expect(updated?.done).toBe(true);
  });

  it("训练入口的题量不超过题库实际容量", async () => {
    const options = await trainingService.getTrainingOptions();
    expect(options.ok).toBe(true);
    if (!options.ok) return;
    expect(options.data.every((option) => option.itemCount >= 1)).toBe(true);
    expect(options.data.some((option) => option.itemCount < 5)).toBe(true);
  });
});
