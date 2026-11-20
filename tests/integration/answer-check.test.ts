import { beforeEach, describe, expect, it } from "vitest";

import { trainingService } from "@/services/api";
import { createMemoryRepository } from "@/services/data/repository";
import { resetLearningStore, setLearningRepository } from "@/stores/learning-store";
import type { Exercise } from "@/types";

/**
 * 2.2.2 的护栏：单题反馈走"统一判分入口"，页面不再自己判分。
 *
 * 注意：这里断言的是**接口形状与判定结果**，不是某个具体分数。
 * backend 模式下同一个函数的实现换成 Rust `training_check`，
 * 因此这组测试同时约定了两端的契约。
 */
function correctOptionId(exercise: Exercise): string {
  const options = (exercise as unknown as { options?: { id: string; correct?: boolean }[] })
    .options;
  const hit = options?.find((option) => option.correct);
  if (!hit) throw new Error("该题没有带 correct 标记的选项");
  return hit.id;
}

describe("训练单题即时校验", () => {
  beforeEach(() => {
    setLearningRepository(createMemoryRepository());
    resetLearningStore();
  });

  it("选对判对，选错判错，返回结构一致", async () => {
    const session = await trainingService.startSession({
      kind: "multiple-choice",
      track: "vocabulary",
      count: 3,
    });
    if (!session.ok) throw new Error(session.error.message);

    const exercise = session.data.exercises[0];
    if (!exercise) throw new Error("会话没有题目");

    const right = await trainingService.checkAnswer({
      sessionId: session.data.id,
      exerciseId: exercise.id,
      value: correctOptionId(exercise),
    });
    if (!right.ok) throw new Error(right.error.message);
    expect(right.data.exerciseId).toBe(exercise.id);
    expect(right.data.correct).toBe(true);
    expect(right.data.partialScore).toBe(1);

    const wrong = await trainingService.checkAnswer({
      sessionId: session.data.id,
      exerciseId: exercise.id,
      value: "__definitely_wrong__",
    });
    if (!wrong.ok) throw new Error(wrong.error.message);
    expect(wrong.data.correct).toBe(false);
    expect(wrong.data.partialScore).toBe(0);
  });

  it("题目不存在时返回错误而不是静默判错", async () => {
    const response = await trainingService.checkAnswer({
      sessionId: "s_not_exist",
      exerciseId: "ex_not_exist",
      value: "a",
    });
    expect(response.ok).toBe(false);
  });
});
