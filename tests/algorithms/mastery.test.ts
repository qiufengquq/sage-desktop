import { describe, expect, it } from "vitest";
import { decayMastery, updateMastery } from "@/lib/algorithms/mastery";
import { entityState, NOW } from "../helpers/fixtures";

describe("掌握度更新", () => {
  it("首次答对不会直接到 100", () => {
    const result = updateMastery({
      snapshot: entityState(),
      partialScore: 1,
      difficulty: 3,
      now: NOW,
    });

    expect(result.next).toBeGreaterThan(20);
    expect(result.next).toBeLessThan(100);
    expect(result.reviewCount).toBe(1);
    expect(result.state).toBe("learning");
  });

  it("连续答对且达到阈值后标记为已掌握", () => {
    let state = entityState({ mastery: 70, reviewCount: 2, state: "reviewing" });

    for (let i = 0; i < 4; i += 1) {
      const update = updateMastery({
        snapshot: state,
        partialScore: 1,
        difficulty: 4,
        now: NOW,
      });
      state = entityState({
        mastery: update.next,
        reviewCount: update.reviewCount,
        state: update.state,
        strength: update.strength,
        lapseCount: update.lapseCount,
        lastReviewedAt: NOW.toISOString(),
      });
    }

    expect(state.mastery).toBeGreaterThanOrEqual(80);
    expect(state.state).toBe("mastered");
  });

  it("答错会降低掌握度", () => {
    const result = updateMastery({
      snapshot: entityState({
        mastery: 80,
        state: "mastered",
        reviewCount: 5,
        lastReviewedAt: NOW.toISOString(),
      }),
      partialScore: 0,
      difficulty: 3,
      now: NOW,
    });

    expect(result.next).toBeLessThan(80);
    expect(result.lapseCount).toBe(1);
  });

  it("已掌握的条目答错会被标记为遗忘", () => {
    const result = updateMastery({
      snapshot: entityState({
        mastery: 90,
        state: "mastered",
        reviewCount: 6,
        lastReviewedAt: NOW.toISOString(),
      }),
      partialScore: 0,
      difficulty: 3,
      now: NOW,
    });

    expect(result.lapsed).toBe(true);
    expect(result.state).toBe("lapsed");
  });

  it("间隔过久会先衰减，并在因子中体现", () => {
    const longAgo = new Date(NOW.getTime() - 30 * 86400000).toISOString();
    const snapshot = entityState({
      mastery: 80,
      reviewCount: 4,
      state: "reviewing",
      lastReviewedAt: longAgo,
    });

    expect(decayMastery(snapshot, NOW)).toBeLessThan(80);

    const result = updateMastery({ snapshot, partialScore: 1, difficulty: 3, now: NOW });
    expect(result.factors.some((f) => f.key === "recency")).toBe(true);
    expect(result.factors.find((f) => f.key === "recency")?.impact).toBeLessThan(0);
  });

  it("同样答对时，难题带来的增益更高", () => {
    const easy = updateMastery({
      snapshot: entityState({ mastery: 30 }),
      partialScore: 1,
      difficulty: 1,
      now: NOW,
    });
    const hard = updateMastery({
      snapshot: entityState({ mastery: 30 }),
      partialScore: 1,
      difficulty: 5,
      now: NOW,
    });

    expect(hard.delta).toBeGreaterThan(easy.delta);
  });

  it("因子影响之和等于掌握度变化（可解释性）", () => {
    const result = updateMastery({
      snapshot: entityState({ mastery: 40, lastReviewedAt: new Date(NOW.getTime() - 20 * 86400000).toISOString() }),
      partialScore: 0.85,
      difficulty: 4,
      now: NOW,
    });

    const total = result.factors.reduce((acc, factor) => acc + factor.impact, 0);
    expect(Math.abs(total - result.delta)).toBeLessThan(0.4);
  });

  it("记忆强度随表现为 0–1", () => {
    const result = updateMastery({
      snapshot: entityState({ strength: 0.2 }),
      partialScore: 1,
      difficulty: 3,
      now: NOW,
    });

    expect(result.strength).toBeGreaterThan(0.2);
    expect(result.strength).toBeLessThanOrEqual(1);
  });
});
