import type { Exercise, ExerciseAnswer } from "@/types";
import { normalizeAnswer } from "./utils";

/**
 * 判分规则集中在这里，前端即时反馈与后端裁定共用同一套逻辑。
 * 第二阶段迁移到 Rust 时，这份规则将成为对照实现（golden implementation）。
 */

export function similarity(a: string, b: string) {
  const wa = normalizeAnswer(a).split(" ").filter(Boolean);
  const wb = normalizeAnswer(b).split(" ").filter(Boolean);
  if (wa.length === 0 || wb.length === 0) return 0;
  const setB = new Set(wb);
  const hits = wa.filter((w) => setB.has(w)).length;
  return hits / Math.max(wa.length, wb.length);
}

/**
 * 单字符层面的相似度（双字符组 Dice 系数）。
 * 用于拼写/听写：用户少写一个字母时应该拿到部分分，而不是 0 分。
 */
export function characterSimilarity(a: string, b: string) {
  const x = normalizeAnswer(a).replace(/\s+/g, "");
  const y = normalizeAnswer(b).replace(/\s+/g, "");
  if (!x || !y) return 0;
  if (x === y) return 1;

  const bigramsOf = (value: string) => {
    const counts = new Map<string, number>();
    for (let i = 0; i < value.length - 1; i += 1) {
      const gram = value.slice(i, i + 2);
      counts.set(gram, (counts.get(gram) ?? 0) + 1);
    }
    return counts;
  };

  const xGrams = bigramsOf(x);
  const yGrams = bigramsOf(y);
  if (xGrams.size === 0 || yGrams.size === 0) return 0;

  let intersection = 0;
  for (const [gram, count] of xGrams) {
    const other = yGrams.get(gram) ?? 0;
    intersection += Math.min(count, other);
  }

  const total = (x.length - 1) + (y.length - 1);
  return total === 0 ? 0 : (2 * intersection) / total;
}

export interface ScoreOutcome {
  correct: boolean;
  /** 0–1，用于部分给分 */
  partial: number;
}

export function scoreAnswer(exercise: Exercise, value: ExerciseAnswer["value"]): ScoreOutcome {
  switch (exercise.kind) {
    case "word-match": {
      const mapping = (typeof value === "object" && !Array.isArray(value) ? value : {}) as Record<
        string,
        string
      >;
      const pairs = exercise.pairs ?? [];
      const total = pairs.length;
      const hits = pairs.filter((p) => mapping[p.left] === p.right).length;
      return { correct: hits === total, partial: total === 0 ? 0 : hits / total };
    }

    case "multiple-choice": {
      const chosen = String(value ?? "");
      const option = exercise.options.find((o) => o.id === chosen || o.label === chosen);
      return { correct: Boolean(option?.correct), partial: option?.correct ? 1 : 0 };
    }

    case "cloze": {
      const chosen = String(value ?? "");
      const option = exercise.options.find((o) => o.id === chosen || o.label === chosen);
      const answer = exercise.answer ?? "";
      const correct = Boolean(option?.correct) || normalizeAnswer(chosen) === normalizeAnswer(answer);
      return { correct, partial: correct ? 1 : 0 };
    }

    case "spelling":
    case "dictation": {
      const answer = exercise.answer ?? "";
      const typed = normalizeAnswer(String(value ?? ""));
      const correct = typed === normalizeAnswer(answer);
      return {
        correct,
        partial: correct ? 1 : characterSimilarity(String(value ?? ""), answer) * 0.6,
      };
    }

    case "translate":
    case "rewrite": {
      const typed = String(value ?? "");
      const candidates = [exercise.answer ?? "", ...(exercise.references ?? [])];
      const best = Math.max(...candidates.map((c) => similarity(typed, c)));
      const exact = candidates.some((c) => normalizeAnswer(c) === normalizeAnswer(typed));
      return { correct: exact || best >= 0.85, partial: exact ? 1 : best };
    }

    case "sentence-order": {
      const expected = (exercise.answer ?? []).join(" ");
      const ordered = Array.isArray(value) ? value.join(" ") : String(value ?? "");
      const correct = normalizeAnswer(ordered) === normalizeAnswer(expected);
      return { correct, partial: correct ? 1 : similarity(ordered, expected) };
    }

    case "error-correction": {
      const answer = exercise.answer ?? "";
      const typed = String(value ?? "");
      const correct = normalizeAnswer(typed) === normalizeAnswer(answer);
      return { correct, partial: correct ? 1 : similarity(typed, answer) };
    }

    case "shadowing": {
      const typed = String(value ?? "");
      const partial = similarity(typed, exercise.text);
      // 不再给"开口即 0.45 分"的保底：没有有效内容就是 0 分，
      // 否则掌握度会把空答案当成部分掌握。
      return { correct: partial >= 0.8, partial };
    }

    case "roleplay": {
      const answers = (typeof value === "object" && !Array.isArray(value) ? value : {}) as Record<
        string,
        string
      >;
      const userTurns = exercise.turns.filter((t) => t.speaker === "user");
      const hits = userTurns.filter((turn) => {
        const said = answers[turn.id] ?? "";
        if (!said.trim()) return false;
        const norm = normalizeAnswer(said);
        return (turn.expected ?? []).some((k) => norm.includes(normalizeAnswer(k)));
      }).length;
      const partial = userTurns.length === 0 ? 0 : hits / userTurns.length;
      return { correct: hits === userTurns.length, partial };
    }

    default:
      return { correct: false, partial: 0 };
  }
}

/**
 * 提取"可读答案"，对应 Rust 侧 `grading::answer_of`。
 *
 * 用途：mock 模式填充 `AnswerCheck.correctAnswer`，让本地反馈与后端同形；
 * 页面据此显示"正确答案/参考表达"，不再从题目对象里读答案。
 */
export function answerOf(exercise: Exercise): unknown {
  switch (exercise.kind) {
    case "multiple-choice":
    case "cloze": {
      const labels = exercise.options.filter((option) => option.correct).map((o) => o.label);
      if (labels.length === 0) {
        // 只有 cloze 带 answer，multiple-choice 没有
        return "answer" in exercise ? (exercise.answer ?? null) : null;
      }
      return labels.length === 1 ? labels[0] : labels;
    }

    case "spelling":
    case "dictation":
    case "error-correction":
    case "translate":
    case "rewrite":
    case "sentence-order":
      return exercise.answer ?? null;

    case "word-match":
      return exercise.pairs ?? null;

    case "shadowing":
      return exercise.text;

    case "roleplay":
      return exercise.turns
        .filter((turn) => turn.speaker === "user")
        .map((turn) => ({ id: turn.id, expected: turn.expected ?? [] }));

    default:
      return null;
  }
}
