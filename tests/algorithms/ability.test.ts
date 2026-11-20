import { describe, expect, it } from "vitest";
import type { UserProfile } from "@/types";
import { buildAbilityProfile } from "@/lib/algorithms/ability";
import { NOW, assessmentAttempt, entityState, trainingAttempt } from "../helpers/fixtures";

const profile: UserProfile = {
  id: "u_1",
  displayName: "测试用户",
  handle: "@test",
  joinedAt: NOW.toISOString(),
  level: "B1",
  targetLevel: "B2",
  goal: "exam",
  dailyMinutes: 40,
  weeklyDays: 6,
  timezone: "Asia/Shanghai",
};

describe("能力画像", () => {
  it("没有任何数据时使用基线，并全部标记为估算", () => {
    const ability = buildAbilityProfile({
      profile,
      assessmentAttempts: [],
      trainingAttempts: [],
      states: {},
      now: NOW,
    });

    expect(ability.axes).toHaveLength(6);
    expect(ability.axes.every((axis) => axis.confidence === "estimated")).toBe(true);
    expect(ability.strengths).toHaveLength(2);
    expect(ability.weaknesses).toHaveLength(2);
    expect(ability.sourceAssessmentId).toBeNull();
  });

  it("有测评时按 60/40 混合测评成绩与掌握度", () => {
    const ability = buildAbilityProfile({
      profile,
      assessmentAttempts: [assessmentAttempt({ moduleId: "vocabulary", score: 80 })],
      trainingAttempts: [],
      states: {
        w_a: entityState({ entityId: "w_a", mastery: 50 }),
        w_b: entityState({ entityId: "w_b", mastery: 50 }),
      },
      now: NOW,
    });

    const vocabulary = ability.axes.find((axis) => axis.key === "vocabulary");
    expect(vocabulary?.score).toBe(68);
    expect(vocabulary?.confidence).toBe("measured");
    expect(vocabulary?.source).toContain("词汇测评");
  });

  it("同一模块多次测评时取最新一次", () => {
    const ability = buildAbilityProfile({
      profile,
      assessmentAttempts: [
        assessmentAttempt({ id: "aa_old", score: 40, finishedAt: new Date(NOW.getTime() - 86400000).toISOString() }),
        assessmentAttempt({ id: "aa_new", score: 90, finishedAt: NOW.toISOString() }),
      ],
      trainingAttempts: [],
      states: {},
      now: NOW,
    });

    const vocabulary = ability.axes.find((axis) => axis.key === "vocabulary");
    expect(vocabulary?.score).toBe(90);
    expect(ability.sourceAssessmentId).toBe("aa_new");
  });

  it("听力/阅读/写作在训练样本足够时由题型正确率估算", () => {
    const ability = buildAbilityProfile({
      profile,
      assessmentAttempts: [],
      trainingAttempts: [
        trainingAttempt({
          perKind: [{ kind: "dictation", correct: 9, total: 10 }],
          answers: [],
        }),
        trainingAttempt({
          id: "ta_2",
          perKind: [{ kind: "cloze", correct: 6, total: 10 }],
          answers: [],
        }),
        trainingAttempt({
          id: "ta_3",
          perKind: [{ kind: "rewrite", correct: 7, total: 10 }],
          answers: [],
        }),
      ],
      states: {},
      now: NOW,
    });

    expect(ability.axes.find((a) => a.key === "listening")?.score).toBe(90);
    expect(ability.axes.find((a) => a.key === "reading")?.score).toBe(60);
    expect(ability.axes.find((a) => a.key === "writing")?.score).toBe(70);
    expect(ability.axes.find((a) => a.key === "listening")?.confidence).toBe("estimated");
  });

  it("综合分是各轴加权平均", () => {
    const ability = buildAbilityProfile({
      profile,
      assessmentAttempts: [],
      trainingAttempts: [],
      states: {},
      now: NOW,
    });

    const expected = Math.round(
      ability.axes.reduce((acc, axis) => {
        const weight =
          axis.key === "vocabulary" || axis.key === "grammar" || axis.key === "speaking"
            ? 0.2
            : axis.key === "listening" || axis.key === "reading"
              ? 0.15
              : 0.1;
        return acc + axis.score * weight;
      }, 0),
    );
    expect(ability.overall).toBe(expected);
  });
});
