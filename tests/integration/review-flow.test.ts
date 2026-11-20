import { beforeEach, describe, expect, it } from "vitest";
import { createMemoryRepository } from "@/services/data/repository";
import { commitDb, getDb, resetLearningStore, setLearningRepository } from "@/stores/learning-store";
import { reviewService } from "@/services/api";
import { buildDashboardData, dueItems } from "@/services/data/selectors";

const DAY = 86400000;

/** 让某个条目立刻到期，保证测试不依赖种子数据的到期时间 */
function forceDue(entityId: string, mastery = 40) {
  commitDb((db) => {
    const state = db.entityStates[entityId];
    if (!state) return db;
    return {
      ...db,
      entityStates: {
        ...db.entityStates,
        [entityId]: {
          ...state,
          dueAt: new Date(Date.now() - DAY).toISOString(),
          mastery,
          state: mastery >= 80 ? "mastered" : "reviewing",
          reviewCount: 2,
          repetition: 1,
          intervalDays: 3,
        },
      },
    };
  });
}

describe("复习闭环（集成）", () => {
  beforeEach(() => {
    setLearningRepository(createMemoryRepository());
    resetLearningStore(new Date());
  });

  it("到期条目进入队列，复习后掌握度更新、下次到期时间推后、日志入库", async () => {
    const db = getDb();
    const entityId = Object.keys(db.entityStates).find((id) => id.startsWith("w_"));
    if (!entityId) throw new Error("seed 缺少词条");

    forceDue(entityId);

    const queue = await reviewService.getReviewQueue(20);
    if (!queue.ok) throw new Error("queue failed");
    expect(queue.data.cards.some((card) => card.item.entityId === entityId)).toBe(true);
    expect(queue.data.dueCount).toBeGreaterThan(0);

    const card = queue.data.cards.find((item) => item.item.entityId === entityId);
    expect(card?.answer.length).toBeGreaterThan(0);
    expect(card?.preview.good).toContain("天");

    const before = getDb().entityStates[entityId];
    const logsBefore = getDb().reviewLogs.length;

    const result = await reviewService.submitReview({ entityId, rating: "good" });
    if (!result.ok) throw new Error(result.error.message);

    const after = getDb().entityStates[entityId];
    expect(after?.mastery).toBeGreaterThan(before?.mastery ?? 0);
    expect(new Date(after?.dueAt ?? 0).getTime()).toBeGreaterThan(Date.now());
    expect(getDb().reviewLogs).toHaveLength(logsBefore + 1);
    expect(result.data.intervalDays).toBeGreaterThanOrEqual(1);
    expect(result.data.xp).toBeGreaterThan(0);

    const stillDue = dueItems(getDb(), new Date()).map((state) => state.entityId);
    expect(stillDue).not.toContain(entityId);
  });

  it("标记忘记：进入重学阶段，10 分钟后再出现", async () => {
    const entityId = Object.keys(getDb().entityStates).find((id) => id.startsWith("w_"));
    if (!entityId) throw new Error("seed 缺少词条");
    forceDue(entityId, 85);

    const result = await reviewService.submitReview({ entityId, rating: "again" });
    if (!result.ok) throw new Error(result.error.message);

    expect(result.data.intervalDays).toBe(0);
    const delta = new Date(result.data.nextDueAt).getTime() - Date.now();
    expect(delta).toBeLessThanOrEqual(11 * 60 * 1000);
    expect(getDb().entityStates[entityId]?.state).toBe("lapsed");
  });

  it("复习计入今日统计并反映在首页", async () => {
    const entityId = Object.keys(getDb().entityStates).find((id) => id.startsWith("w_"));
    if (!entityId) throw new Error("seed 缺少词条");
    forceDue(entityId);

    const before = buildDashboardData(getDb(), new Date()).snapshot.today.xp;
    await reviewService.submitReview({ entityId, rating: "easy" });
    const after = buildDashboardData(getDb(), new Date()).snapshot.today.xp;

    expect(after).toBeGreaterThan(before);
  });
});
