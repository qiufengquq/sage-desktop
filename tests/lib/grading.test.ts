import { describe, expect, it } from "vitest";
import type {
  ChoiceExercise,
  ClozeExercise,
  CorrectionExercise,
  DictationExercise,
  MatchExercise,
  OrderExercise,
  RoleplayExercise,
  RewriteExercise,
  ShadowingExercise,
  SpellingExercise,
  TranslateExercise,
} from "@/types";
import { scoreAnswer, similarity } from "@/lib/grading";

const base = {
  difficulty: 3 as const,
  entityIds: ["w_test"],
  explanation: "解析",
  xp: 10,
  tags: ["测试"],
};

const prompt = { instruction: "作答" };

describe("判分：11 种题型", () => {
  it("word-match 按配对正确数给部分分", () => {
    const exercise: MatchExercise = {
      ...base,
      id: "ex_match",
      kind: "word-match",
      prompt,
      pairs: [
        { id: "p1", left: "mitigate", right: "减轻" },
        { id: "p2", left: "coherent", right: "连贯的" },
      ],
    };

    expect(scoreAnswer(exercise, { mitigate: "减轻", coherent: "连贯的" })).toEqual({
      correct: true,
      partial: 1,
    });
    expect(scoreAnswer(exercise, { mitigate: "减轻" }).partial).toBe(0.5);
    expect(scoreAnswer(exercise, "").correct).toBe(false);
  });

  it("multiple-choice 支持选项 id 与文本", () => {
    const exercise: ChoiceExercise = {
      ...base,
      id: "ex_choice",
      kind: "multiple-choice",
      prompt,
      stem: "____ the impact",
      options: [
        { id: "o1", label: "mitigate", correct: true },
        { id: "o2", label: "accumulate", correct: false },
      ],
    };

    expect(scoreAnswer(exercise, "o1")).toEqual({ correct: true, partial: 1 });
    expect(scoreAnswer(exercise, "mitigate").correct).toBe(true);
    expect(scoreAnswer(exercise, "o2").correct).toBe(false);
    expect(scoreAnswer(exercise, "").correct).toBe(false);
  });

  it("cloze 支持选项与自由输入", () => {
    const exercise: ClozeExercise = {
      ...base,
      id: "ex_cloze",
      kind: "cloze",
      prompt,
      sentence: "She was ______ to admit it.",
      options: [
        { id: "o1", label: "reluctant", correct: true },
        { id: "o2", label: "resilient", correct: false },
      ],
      answer: "reluctant",
    };

    expect(scoreAnswer(exercise, "o1").correct).toBe(true);
    expect(scoreAnswer(exercise, "Reluctant").correct).toBe(true);
    expect(scoreAnswer(exercise, "resilient").correct).toBe(false);
  });

  it("spelling 精确匹配得满分，拼错按相似度给部分分", () => {
    const exercise: SpellingExercise = {
      ...base,
      id: "ex_spell",
      kind: "spelling",
      prompt,
      definition: "减轻；缓和",
      answer: "mitigate",
      hint: "m______",
    };

    expect(scoreAnswer(exercise, " Mitigate ").partial).toBe(1);
    expect(scoreAnswer(exercise, "mitigat").partial).toBeGreaterThan(0);
    expect(scoreAnswer(exercise, "mitigat").partial).toBeLessThan(1);
    expect(scoreAnswer(exercise, "banana").partial).toBe(0);
  });

  it("dictation 与 spelling 规则一致", () => {
    const exercise: DictationExercise = {
      ...base,
      id: "ex_dict",
      kind: "dictation",
      prompt,
      audioText: "coherent",
      answer: "coherent",
      hint: "c______",
    };

    expect(scoreAnswer(exercise, "coherent").correct).toBe(true);
    expect(scoreAnswer(exercise, "cohrent").partial).toBeLessThan(1);
  });

  it("translate 接受参考答案，达到阈值算对", () => {
    const exercise: TranslateExercise = {
      ...base,
      id: "ex_trans",
      kind: "translate",
      prompt,
      source: "我们需要考虑成本。",
      sourceLang: "zh",
      targetLang: "en",
      answer: "We need to take the cost into account.",
      references: ["We have to consider the cost."],
    };

    expect(scoreAnswer(exercise, "we have to consider the cost").correct).toBe(true);
    expect(scoreAnswer(exercise, "the cost").partial).toBeGreaterThan(0);
    expect(scoreAnswer(exercise, "").partial).toBe(0);
  });

  it("sentence-order 按词序比对", () => {
    const exercise: OrderExercise = {
      ...base,
      id: "ex_order",
      kind: "sentence-order",
      prompt,
      tokens: ["was", "The", "book", "short"],
      answer: ["The", "book", "was", "short"],
    };

    expect(scoreAnswer(exercise, ["The", "book", "was", "short"]).correct).toBe(true);
    expect(scoreAnswer(exercise, ["The", "book", "short", "was"]).correct).toBe(false);
    expect(scoreAnswer(exercise, ["The", "book", "short", "was"]).partial).toBeGreaterThan(0);
  });

  it("error-correction 要求完整改正", () => {
    const exercise: CorrectionExercise = {
      ...base,
      id: "ex_corr",
      kind: "error-correction",
      prompt,
      wrong: "He go to school every day.",
      answer: "He goes to school every day.",
      errorTag: "主谓一致",
    };

    expect(scoreAnswer(exercise, "He goes to school every day.").correct).toBe(true);
    expect(scoreAnswer(exercise, "He go to schools every day.").partial).toBeGreaterThan(0);
    expect(scoreAnswer(exercise, "He go to school every day.").partial).toBeLessThan(1);
  });

  it("rewrite 与 translate 共用相似度规则", () => {
    const exercise: RewriteExercise = {
      ...base,
      id: "ex_rewrite",
      kind: "rewrite",
      prompt,
      source: "The teacher gave us homework.",
      instruction: "改为被动语态",
      answer: "We were given homework by the teacher.",
      references: [],
    };

    expect(scoreAnswer(exercise, "We were given homework by the teacher").correct).toBe(true);
    expect(scoreAnswer(exercise, "The teacher gave us homework").partial).toBeGreaterThan(0);
  });

  it("shadowing 不再给空答案保底分", () => {
    const exercise: ShadowingExercise = {
      ...base,
      id: "ex_shadow",
      kind: "shadowing",
      prompt,
      text: "Small habits compound over time.",
      translation: "小习惯会随时间复利。",
      chunks: ["Small habits", "compound", "over time."],
      targetSeconds: 4,
    };

    expect(scoreAnswer(exercise, "small habits compound over time").correct).toBe(true);
    expect(scoreAnswer(exercise, "").partial).toBe(0);
    expect(scoreAnswer(exercise, "small habits").partial).toBeLessThan(0.8);
  });

  it("roleplay 按用户轮次是否命中关键词给分", () => {
    const exercise: RoleplayExercise = {
      ...base,
      id: "ex_role",
      kind: "roleplay",
      prompt,
      scenario: "调整会议时间",
      turns: [
        { id: "t1", speaker: "ai", en: "Could we move the meeting?", zh: "我们能把会议改期吗？" },
        {
          id: "t2",
          speaker: "user",
          en: "Sure, how about Friday?",
          zh: "可以，周五怎么样？",
          expected: ["Friday", "how about"],
        },
        { id: "t3", speaker: "user", en: "Thanks.", zh: "谢谢。", expected: ["Thanks"] },
      ],
    };

    expect(scoreAnswer(exercise, { t2: "How about Friday?", t3: "Thanks" })).toEqual({
      correct: true,
      partial: 1,
    });
    expect(scoreAnswer(exercise, { t2: "Friday works" }).partial).toBe(0.5);
    expect(scoreAnswer(exercise, {}).partial).toBe(0);
  });

  it("相似度对大小写、标点与多余空格不敏感", () => {
    expect(similarity(" Hello, World! ", "hello world")).toBe(1);
  });
});
