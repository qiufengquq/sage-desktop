/**
 * 生成双端一致性用例：用 TS 实现（对照基准）产出期望值，
 * 供 vitest 与 cargo test 同时消费。
 *
 *   node scripts/gen-parity.mjs
 *
 * 只要 TS 侧行为变化，重新生成即可；Rust 侧若与其不一致，cargo test 会失败。
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { register } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..");
const OUT = resolve(ROOT, "tests/fixtures/parity/algorithms.json");

globalThis.__SAGE_ENV__ = {};
register("./alias-loader.mjs", pathToFileURL(`${HERE}/`));

const { scoreAnswer } = await import(pathToFileURL(resolve(ROOT, "src/lib/grading.ts")).href);
const { updateMastery } = await import(pathToFileURL(resolve(ROOT, "src/lib/algorithms/mastery.ts")).href);
const { scheduleNext } = await import(pathToFileURL(resolve(ROOT, "src/lib/algorithms/srs.ts")).href);
const { levelFromXp, xpForReview } = await import(pathToFileURL(resolve(ROOT, "src/lib/algorithms/xp.ts")).href);
const { selectExercises } = await import(pathToFileURL(resolve(ROOT, "src/lib/algorithms/selection.ts")).href);

/* -------------------------------- 判分用例 -------------------------------- */

const gradingCases = [
  {
    kind: "word-match",
    payload: { pairs: [{ id: "p1", left: "apple", right: "苹果" }, { id: "p2", left: "banana", right: "香蕉" }] },
    value: { apple: "苹果", banana: "香蕉" },
  },
  {
    kind: "word-match",
    payload: { pairs: [{ id: "p1", left: "apple", right: "苹果" }, { id: "p2", left: "banana", right: "香蕉" }] },
    value: { apple: "苹果", banana: "苹果" },
  },
  {
    kind: "multiple-choice",
    payload: { stem: "选一个", options: [{ id: "a", label: "苹果", correct: true }, { id: "b", label: "香蕉", correct: false }] },
    value: "b",
  },
  {
    kind: "multiple-choice",
    payload: { stem: "选一个", options: [{ id: "a", label: "苹果", correct: true }] },
    value: "苹果",
  },
  {
    kind: "cloze",
    payload: { sentence: "I ___ tea", options: [{ id: "a", label: "drink", correct: true }], answer: "drink" },
    value: "drink",
  },
  { kind: "spelling", payload: { definition: "苹果", answer: "apple", hint: "a____" }, value: "Apple" },
  { kind: "spelling", payload: { definition: "苹果", answer: "apple", hint: "a____" }, value: "appl" },
  { kind: "dictation", payload: { audioText: "good morning", answer: "good morning", hint: "" }, value: "good mornin" },
  {
    kind: "translate",
    payload: { source: "我喜欢茶", sourceLang: "zh", targetLang: "en", answer: "I like tea", references: ["I love tea"] },
    value: "I like tea",
  },
  {
    kind: "translate",
    payload: { source: "我喜欢茶", sourceLang: "zh", targetLang: "en", answer: "I like tea", references: ["I love tea"] },
    value: "I like coffee",
  },
  {
    kind: "sentence-order",
    payload: { tokens: ["tea", "I", "like"], answer: ["I", "like", "tea"] },
    value: ["I", "like", "tea"],
  },
  {
    kind: "sentence-order",
    payload: { tokens: ["tea", "I", "like"], answer: ["I", "like", "tea"] },
    value: ["I", "tea", "like"],
  },
  {
    kind: "error-correction",
    payload: { wrong: "He go to school", answer: "He goes to school", errorTag: "subject-verb-agreement" },
    value: "He goes to school",
  },
  {
    kind: "rewrite",
    payload: { source: "The book was read by me", instruction: "改成主动语态", answer: "I read the book", references: [] },
    value: "I read the book",
  },
  {
    kind: "shadowing",
    payload: { text: "I would like a cup of tea", translation: "我想要一杯茶", chunks: [], targetSeconds: 4 },
    value: "I would like a cup of tea",
  },
  {
    kind: "shadowing",
    payload: { text: "I would like a cup of tea", translation: "我想要一杯茶", chunks: [], targetSeconds: 4 },
    value: "",
  },
  {
    kind: "roleplay",
    payload: {
      scenario: "点单",
      turns: [
        { id: "t1", speaker: "ai", en: "Hi", zh: "嗨" },
        { id: "t2", speaker: "user", en: "A tea please", zh: "一杯茶", expected: ["tea"] },
        { id: "t3", speaker: "user", en: "Thanks", zh: "谢谢", expected: ["thank"] },
      ],
    },
    value: { t2: "A tea please", t3: "Thanks" },
  },
];

const grading = gradingCases.map((item) => {
  // scoreAnswer 读的是"摊平后的题目"，payload 需要展开到顶层
  const outcome = scoreAnswer({ ...item.payload, kind: item.kind }, item.value);
  return { ...item, expect: { correct: outcome.correct, partial: outcome.partial } };
});

/* ------------------------------- 掌握度用例 ------------------------------- */

const masterySnapshots = [
  { mastery: 0, strength: 0, state: "new", reviewCount: 0, lapseCount: 0, lastReviewedAt: "2026-11-13T00:00:00.000Z" },
  { mastery: 42.5, strength: 0.35, state: "reviewing", reviewCount: 4, lapseCount: 1, lastReviewedAt: "2026-11-13T00:00:00.000Z" },
  { mastery: 85, strength: 0.8, state: "mastered", reviewCount: 9, lapseCount: 0, lastReviewedAt: "2026-09-01T00:00:00.000Z" },
  { mastery: 60, strength: 0.5, state: "learning", reviewCount: 2, lapseCount: 0, lastReviewedAt: null },
];

const masteryInputs = [
  { partial: 1, difficulty: 3, now: "2026-11-14T00:00:00.000Z" },
  { partial: 0.75, difficulty: 5, now: "2026-11-14T00:00:00.000Z" },
  { partial: 0.4, difficulty: 2, now: "2026-12-20T00:00:00.000Z" },
  { partial: 0, difficulty: 3, now: "2026-11-14T00:00:00.000Z" },
];

const mastery = [];
for (const snapshot of masterySnapshots) {
  for (const input of masteryInputs) {
    const update = updateMastery({
      snapshot,
      partialScore: input.partial,
      difficulty: input.difficulty,
      now: new Date(input.now),
    });
    mastery.push({
      snapshot,
      partialScore: input.partial,
      difficulty: input.difficulty,
      now: input.now,
      expect: {
        previous: update.previous,
        next: update.next,
        delta: update.delta,
        strength: update.strength,
        state: update.state,
        reviewCount: update.reviewCount,
        lapseCount: update.lapseCount,
        lapsed: update.lapsed,
      },
    });
  }
}

/* ------------------------------- 调度用例 -------------------------------- */

const srsStates = [
  { ease: 2.5, intervalDays: 0, repetition: 0, lapseCount: 0 },
  { ease: 2.5, intervalDays: 1, repetition: 1, lapseCount: 0 },
  { ease: 2.5, intervalDays: 10, repetition: 3, lapseCount: 1 },
  { ease: 2.8, intervalDays: 170, repetition: 6, lapseCount: 0 },
];

const srs = [];
for (const state of srsStates) {
  for (const rating of ["again", "hard", "good", "easy"]) {
    const outcome = scheduleNext(state, rating, new Date("2026-11-14T08:30:00.000Z"));
    srs.push({
      state,
      rating,
      now: "2026-11-14T08:30:00.000Z",
      expect: {
        ease: outcome.ease,
        intervalDays: outcome.intervalDays,
        repetition: outcome.repetition,
        lapseCount: outcome.lapseCount,
        dueAt: outcome.dueAt,
      },
    });
  }
}

/* --------------------------------- 经验 ---------------------------------- */

const xp = [0, 120, 499, 500, 1059, 1060, 5000].map((total) => ({
  total,
  expect: levelFromXp(total),
}));

const reviewXp = ["again", "hard", "good", "easy"].map((rating) => ({
  rating,
  expect: xpForReview(rating),
}));

/* --------------------------------- 取题 ---------------------------------- */

const selectionPool = [
  { id: "ex_1", kind: "spelling", difficulty: 1, prompt: { instruction: "a" }, entityIds: [], explanation: "", xp: 5, tags: [], answer: "a" },
  { id: "ex_2", kind: "spelling", difficulty: 3, prompt: { instruction: "b" }, entityIds: [], explanation: "", xp: 5, tags: [], answer: "b" },
  { id: "ex_3", kind: "spelling", difficulty: 3, prompt: { instruction: "c" }, entityIds: [], explanation: "", xp: 5, tags: [], answer: "c" },
  { id: "ex_4", kind: "spelling", difficulty: 5, prompt: { instruction: "d" }, entityIds: [], explanation: "", xp: 5, tags: [], answer: "d" },
];

const selectionCases = [
  { targetDifficulty: 3, count: 2, seenAt: {} },
  { targetDifficulty: 3, count: 2, seenAt: { ex_2: new Date("2026-11-13T12:00:00.000Z").getTime() } },
  { targetDifficulty: 5, count: 4, seenAt: { ex_4: new Date("2026-11-01T12:00:00.000Z").getTime() } },
];

const selection = selectionCases.map((item) => {
  const seenAt = new Map(Object.entries(item.seenAt));
  const picked = selectExercises({
    pool: selectionPool,
    count: item.count,
    targetDifficulty: item.targetDifficulty,
    seenAt,
    now: new Date("2026-11-14T12:00:00.000Z"),
  });
  return {
    pool: selectionPool.map((exercise) => ({ id: exercise.id, difficulty: exercise.difficulty })),
    targetDifficulty: item.targetDifficulty,
    count: item.count,
    seenAt: item.seenAt,
    now: "2026-11-14T12:00:00.000Z",
    expectIds: picked.map((exercise) => exercise.id),
  };
});

const fixture = { grading, mastery, srs, xp, reviewXp, selection };

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, `${JSON.stringify(fixture, null, 2)}\n`, "utf8");

console.log(
  `[parity] 已生成 ${OUT}\n[parity] grading=${grading.length} mastery=${mastery.length} srs=${srs.length} xp=${xp.length} selection=${selection.length}`,
);
