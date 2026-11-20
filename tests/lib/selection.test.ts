import { describe, expect, it } from "vitest";
import type { Exercise } from "@/types";
import { selectExercises } from "@/lib/algorithms/selection";
import { NOW } from "../helpers/fixtures";

function exercise(id: string, difficulty: 1 | 2 | 3 | 4 | 5): Exercise {
  return {
    id,
    kind: "spelling",
    difficulty,
    entityIds: [`w_${id}`],
    explanation: "",
    xp: 10,
    tags: [],
    prompt: { instruction: "拼写" },
    definition: "释义",
    answer: "answer",
    hint: "",
  };
}

const DAY = 86400000;

describe("取题策略", () => {
  it("题量不超过题库容量", () => {
    const pool = [exercise("a", 3)];
    expect(selectExercises({ pool, count: 5, targetDifficulty: 3, seenAt: new Map(), now: NOW })).toHaveLength(1);
  });

  it("优先选择难度接近目标的题", () => {
    const pool = [exercise("easy", 1), exercise("fit", 3), exercise("hard", 5)];
    const picked = selectExercises({
      pool,
      count: 1,
      targetDifficulty: 3,
      seenAt: new Map(),
      now: NOW,
    });
    expect(picked[0]?.id).toBe("fit");
  });

  it("最近做过的题被降权", () => {
    const pool = [exercise("recent", 3), exercise("fresh", 3)];
    const seenAt = new Map([["recent", NOW.getTime() - DAY * 0.5]]);
    const picked = selectExercises({ pool, count: 1, targetDifficulty: 3, seenAt, now: NOW });
    expect(picked[0]?.id).toBe("fresh");
  });

  it("时间足够久的旧题重新进入候选", () => {
    const pool = [exercise("old", 3), exercise("fresh", 3)];
    const seenAt = new Map([["old", NOW.getTime() - DAY * 30]]);
    const picked = selectExercises({ pool, count: 2, targetDifficulty: 3, seenAt, now: NOW });
    expect(picked).toHaveLength(2);
    expect(picked[0]?.id).toBe("old");
  });

  it("相同输入得到相同顺序（确定性）", () => {
    const pool = [exercise("a", 2), exercise("b", 2), exercise("c", 2)];
    const first = selectExercises({ pool, count: 3, targetDifficulty: 2, seenAt: new Map(), now: NOW });
    const second = selectExercises({ pool, count: 3, targetDifficulty: 2, seenAt: new Map(), now: NOW });
    expect(first.map((item) => item.id)).toEqual(second.map((item) => item.id));
  });
});
