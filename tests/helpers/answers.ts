import type { Exercise } from "@/types";

/** 构造一份"全对"的作答值，用于集成测试 */
export function correctValueFor(exercise: Exercise): string | string[] | Record<string, string> {
  switch (exercise.kind) {
    case "word-match":
      return Object.fromEntries(exercise.pairs.map((pair) => [pair.left, pair.right]));
    case "multiple-choice":
      return exercise.options.find((option) => option.correct)?.id ?? "";
    case "cloze":
      return exercise.answer;
    case "spelling":
    case "dictation":
      return exercise.answer;
    case "translate":
    case "rewrite":
    case "error-correction":
      return exercise.answer;
    case "sentence-order":
      return [...exercise.answer];
    case "shadowing":
      return exercise.text;
    case "roleplay": {
      const entries = exercise.turns
        .filter((turn) => turn.speaker === "user")
        .map((turn) => [turn.id, turn.en] as const);
      return Object.fromEntries(entries);
    }
    default:
      return "";
  }
}
