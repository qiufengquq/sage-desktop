import { describe, expect, it } from "vitest";
import type { AbilityProfile, StudyPlan, UserProfile } from "@/types";
import { buildAbilityProfile } from "@/lib/algorithms/ability";
import { generatePlan } from "@/lib/algorithms/planner";
import { NOW, entityState } from "../helpers/fixtures";

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

function ability(): AbilityProfile {
  return buildAbilityProfile({
    profile,
    assessmentAttempts: [],
    trainingAttempts: [],
    states: {},
    now: NOW,
  });
}

function planInput(overrides: Partial<Parameters<typeof generatePlan>[0]> = {}) {
  return {
    profile,
    ability: ability(),
    states: {},
    dueCount: 0,
    recentAccuracy: null,
    weeklyMinutes: 90,
    weeklyActiveDays: 3,
    weeklyNewMastered: 5,
    weeklyGrammarCorrect: 12,
    difficultyOverride: null,
    generatedBy: "regenerate" as const,
    now: NOW,
    previous: null,
    ...overrides,
  };
}

describe("学习计划生成", () => {
  it("没有到期条目时不生成复习任务", () => {
    const plan = generatePlan(planInput());
    expect(plan.today.items.some((item) => item.action.type === "review")).toBe(false);
  });

  it("有到期条目时生成复习任务，时长随时长上限收敛", () => {
    const plan = generatePlan(planInput({ dueCount: 30 }));
    const review = plan.today.items.find((item) => item.action.type === "review");
    expect(review).toBeDefined();
    expect(review?.minutes).toBeGreaterThanOrEqual(5);
    expect(review?.minutes).toBeLessThanOrEqual(15);
    expect(review?.reason).toContain("30");
  });

  it("总时长不超过每日预算", () => {
    const plan = generatePlan(planInput({ dueCount: 8 }));
    expect(plan.today.minutes).toBeLessThanOrEqual(profile.dailyMinutes);
  });

  it("计划项都带可执行 action", () => {
    const plan = generatePlan(planInput({ dueCount: 5 }));
    expect(plan.today.items.length).toBeGreaterThan(1);
    expect(plan.today.items.every((item) => Boolean(item.action.type))).toBe(true);
  });

  it("最弱能力决定专项任务", () => {
    const weakAbility = ability();
    const weakest = [...weakAbility.axes].sort((a, b) => a.score - b.score)[0];
    const plan = generatePlan(planInput({ ability: weakAbility }));
    const focus = plan.today.items.find((item) => item.id.endsWith("_focus"));
    expect(focus?.title).toContain(weakest?.label ?? "");
  });

  it("正确率高于目标区间时提高难度，低于时降低", () => {
    const high = generatePlan(planInput({ recentAccuracy: 95 }));
    const low = generatePlan(planInput({ recentAccuracy: 55 }));
    expect(high.difficulty.suggested).toBeGreaterThan(low.difficulty.suggested);
    expect(high.difficulty.reason).toContain("95");
  });

  it("手动难度优先于建议值", () => {
    const plan = generatePlan(planInput({ recentAccuracy: 95, difficultyOverride: 2.2 }));
    expect(plan.difficulty.current).toBe(2.2);
    expect(plan.difficulty.requiresConfirmation).toBe(false);
  });

  it("重新生成时保留今天已完成的任务", () => {
    const previous = generatePlan(planInput({ dueCount: 4 })) as StudyPlan;
    const marked: StudyPlan = {
      ...previous,
      today: {
        ...previous.today,
        items: previous.today.items.map((item, i) => (i === 0 ? { ...item, done: true, status: "done" as const } : item)),
      },
    };

    const next = generatePlan(planInput({ dueCount: 4, previous: marked }));
    expect(next.today.items[0]?.done).toBe(true);
    expect(next.completion.done).toBeGreaterThanOrEqual(1);
  });

  it("周目标来自用户设置", () => {
    const plan = generatePlan(planInput());
    const minutes = plan.weeklyGoals.find((goal) => goal.id === "wg_minutes");
    expect(minutes?.target).toBe(profile.dailyMinutes * profile.weeklyDays);
    expect(minutes?.current).toBe(90);
  });

  it("建议都带可核对的依据，且模型标识不伪装成 AI", () => {
    const plan = generatePlan(planInput({ dueCount: 3, recentAccuracy: 78 }));
    expect(plan.advice.length).toBeGreaterThanOrEqual(2);
    expect(plan.advice.every((advice) => advice.evidence.length > 0)).toBe(true);
    expect(plan.model).toContain("rule-based");
  });

  it("掌握度参与复习建议的依据", () => {
    const plan = generatePlan(
      planInput({
        dueCount: 2,
        states: {
          w_a: entityState({ entityId: "w_a", mastery: 20 }),
          w_b: entityState({ entityId: "w_b", mastery: 40 }),
        },
      }),
    );
    const reviewAdvice = plan.advice.find((advice) => advice.id === "adv_review");
    expect(reviewAdvice?.evidence.join(" ")).toContain("30");
  });
});
