import { beforeEach, describe, expect, it } from "vitest";
import { createMemoryRepository } from "@/services/data/repository";
import { commitDb, getDb, resetLearningStore, setLearningRepository } from "@/stores/learning-store";
import { assessmentService, plannerService, trainingService } from "@/services/api";
import { buildDashboardData } from "@/services/data/selectors";
import { correctValueFor } from "../helpers/answers";

describe("计划与首页同步（集成）", () => {
  beforeEach(() => {
    setLearningRepository(createMemoryRepository());
    resetLearningStore(new Date());
  });

  it("首页任务与规划页来自同一份计划", async () => {
    const plan = await plannerService.getPlan();
    if (!plan.ok) throw new Error("plan failed");

    const dashboard = buildDashboardData(getDb(), new Date());
    expect(dashboard.tasks.map((task) => task.id)).toEqual(
      plan.data.today.items.map((item) => item.id),
    );
    expect(dashboard.recommendation?.id).toBe(plan.data.advice[0]?.id);
  });

  it("在规划页勾选任务后，首页立即反映完成状态", async () => {
    const plan = getDb().plan;
    // 种子计划里可能有已完成项，这里挑一个未完成的来验证勾选
    const item = plan?.today.items.find((entry) => !entry.done);
    if (!item) throw new Error("计划为空");

    const response = await plannerService.togglePlanItem(item.id);
    expect(response.ok).toBe(true);

    const dashboard = buildDashboardData(getDb(), new Date());
    const task = dashboard.tasks.find((entry) => entry.id === item.id);
    expect(task?.status).toBe("done");
    expect(dashboard.planCompletion).toBeGreaterThan(0);
  });

  it("重新生成计划会按最新画像重算，而不是复制模板", async () => {
    const before = getDb().plan;
    if (!before) throw new Error("计划为空");

    const modules = await assessmentService.getAssessmentModules();
    if (!modules.ok) throw new Error("modules failed");
    const grammar = modules.data.find((module) => module.id === "grammar");
    if (!grammar) throw new Error("缺少语法模块");

    const answers = grammar.sections.flatMap((section) =>
      section.items.map((item) => ({
        exerciseId: item.id,
        value: correctValueFor(item),
        correct: true,
        partialScore: 1,
        elapsedMs: 8000,
      })),
    );
    await assessmentService.submitAssessment({
      moduleId: "grammar",
      answers,
      submissionId: "plan-grammar-1",
    });

    const afterAssessment = getDb().plan;
    expect(afterAssessment?.generatedAt).not.toBe(before.generatedAt);
    expect(afterAssessment?.profile.axes.find((axis) => axis.key === "grammar")?.confidence).toBe(
      "measured",
    );

    const regenerated = await plannerService.regeneratePlan();
    if (!regenerated.ok) throw new Error("regenerate failed");
    expect(regenerated.data.generatedAt).not.toBe(afterAssessment?.generatedAt);
    expect(regenerated.data.today.items.length).toBeGreaterThan(0);
    expect(regenerated.data.today.items.every((item) => Boolean(item.action.type))).toBe(true);
    expect(regenerated.data.weeklyGoals.length).toBe(4);
  });

  it("难度调整会写入计划，并影响下一次取题的难度目标", async () => {
    const applied = await plannerService.applyDifficulty(4.6);
    expect(applied.ok).toBe(true);
    if (!applied.ok) return;

    expect(applied.data.difficulty.current).toBe(4.6);
    expect(applied.data.difficulty.requiresConfirmation).toBe(false);
    expect(getDb().planMeta.difficultyOverride).toBe(4.6);

    const session = await trainingService.startSession({
      kind: "spelling",
      track: "vocabulary",
      count: 1,
    });
    expect(session.ok).toBe(true);
    if (!session.ok) return;

    const picked = session.data.exercises[0];
    expect(picked).toBeDefined();
    // 题库里拼写题难度在 2–4 之间，目标 4.6 时应取到最接近上限的题
    expect(picked?.difficulty).toBeGreaterThanOrEqual(3);
  });

  it("清理本地数据只删除学习记录，保留知识库与资料", async () => {
    const before = getDb();
    const entityCount = Object.keys(before.entityStates).length;

    commitDb((db) => ({
      ...db,
      trainingAttempts: db.trainingAttempts.slice(0, 1),
      reviewLogs: db.reviewLogs,
    }));

    const { userService } = await import("@/services/api");
    const cleared = await userService.clearLocalData();
    expect(cleared.ok).toBe(true);

    const after = getDb();
    expect(after.trainingAttempts).toHaveLength(0);
    expect(after.reviewLogs).toHaveLength(0);
    expect(after.masteryEvents).toHaveLength(0);
    expect(Object.keys(after.entityStates)).toHaveLength(entityCount);
    expect(after.profile.displayName).toBe(before.profile.displayName);
    expect(after.plan).not.toBeNull();
  });
});
