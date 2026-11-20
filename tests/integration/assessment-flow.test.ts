import { beforeEach, describe, expect, it } from "vitest";
import type { AssessmentModuleId, ExerciseAnswer } from "@/types";
import { createMemoryRepository } from "@/services/data/repository";
import { getDb, resetLearningStore, setLearningRepository } from "@/stores/learning-store";
import { assessmentService } from "@/services/api";
import { currentAbility } from "@/services/data/learning-db";
import { correctValueFor } from "../helpers/answers";

async function submitFull(moduleId: AssessmentModuleId, submissionId: string) {
  const modules = await assessmentService.getAssessmentModules();
  if (!modules.ok) throw new Error("modules failed");
  const module = modules.data.find((item) => item.id === moduleId);
  if (!module) throw new Error("module missing");

  const answers: ExerciseAnswer[] = module.sections.flatMap((section) =>
    section.items.map((item) => ({
      exerciseId: item.id,
      value: correctValueFor(item),
      correct: true,
      partialScore: 1,
      elapsedMs: 12000,
    })),
  );

  const outcome = await assessmentService.submitAssessment({ moduleId, answers, submissionId });
  if (!outcome.ok) throw new Error(outcome.error.message);
  return { outcome: outcome.data, answers };
}

describe("测评闭环（集成）", () => {
  beforeEach(() => {
    setLearningRepository(createMemoryRepository());
    resetLearningStore(new Date());
  });

  it("提交测评后：成绩入库、上次成绩更新、画像标记为实测、计划按新画像重排", async () => {
    const before = getDb();
    const beforeAttempts = before.assessmentAttempts.length;
    const beforePlanAt = before.plan?.generatedAt;

    const { outcome } = await submitFull("grammar", "sub-grammar-1");

    const after = getDb();
    expect(after.assessmentAttempts).toHaveLength(beforeAttempts + 1);

    const saved = after.assessmentAttempts[after.assessmentAttempts.length - 1];
    expect(saved?.moduleId).toBe("grammar");
    expect(saved?.score).toBe(100);
    expect(saved?.perSection.every((section) => section.score === 100)).toBe(true);
    expect(saved?.submissionId).toBe("sub-grammar-1");

    const modules = await assessmentService.getAssessmentModules();
    if (!modules.ok) throw new Error("modules failed");
    const grammar = modules.data.find((item) => item.id === "grammar");
    expect(grammar?.lastAttempt?.score).toBe(100);

    const ability = currentAbility(after, new Date());
    const grammarAxis = ability.axes.find((axis) => axis.key === "grammar");
    expect(grammarAxis?.confidence).toBe("measured");
    expect(grammarAxis?.score).toBeGreaterThan(60);

    expect(after.plan?.generatedAt).not.toBe(beforePlanAt);
    expect(after.planMeta.generatedBy).toBe("assessment");
    expect(outcome.followUp).not.toBeNull();
  });

  it("同一 submissionId 重复提交不会重复记录", async () => {
    const modules = await assessmentService.getAssessmentModules();
    if (!modules.ok) throw new Error("modules failed");
    const module = modules.data[0];
    if (!module) throw new Error("module missing");

    const answers: ExerciseAnswer[] = module.sections.flatMap((section) =>
      section.items.map((item) => ({
        exerciseId: item.id,
        value: correctValueFor(item),
        correct: true,
        partialScore: 1,
        elapsedMs: 1000,
      })),
    );

    const first = await assessmentService.submitAssessment({
      moduleId: module.id,
      answers,
      submissionId: "dup-1",
    });
    if (!first.ok) throw new Error(first.error.message);
    expect(first.data.duplicate).toBe(false);

    const afterFirst = getDb().assessmentAttempts.length;

    const second = await assessmentService.submitAssessment({
      moduleId: module.id,
      answers,
      submissionId: "dup-1",
    });
    if (!second.ok) throw new Error(second.error.message);
    // 前后端必须用同一个幂等语义：命中键 → 标记 duplicate 且不新增记录
    expect(second.data.duplicate).toBe(true);
    expect(getDb().assessmentAttempts.length).toBe(afterFirst);
  });

  it("测评记录出现在首页最近记录中", async () => {
    await submitFull("vocabulary", "sub-vocab-1");
    const { buildDashboardData } = await import("@/services/data/selectors");
    const dashboard = buildDashboardData(getDb(), new Date());
    expect(
      dashboard.recentRecords.some((record) => record.activity === "assessment"),
    ).toBe(true);
  });
});
