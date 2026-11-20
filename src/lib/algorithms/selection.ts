import type { Exercise } from "@/types";

/**
 * 取题策略（确定性，可测试）。
 *
 * 目标：
 * 1. 尊重训练入口声明的题量，不假装有 5 题实际只有 1 题。
 * 2. 难度贴近当前计划难度，避免一直做太简单或太难的题。
 * 3. 最近做过的题降权，让同一训练的内容会变化。
 * 4. 结果稳定：同样的输入得到同样的顺序（不引入随机数，便于测试与复现）。
 *
 * 打分（越小越优先）：
 *   难度差 × 2 + 近期惩罚（最近 1 天内 +6，3 天内 +3，7 天内 +1）
 * 同分时按题库原始顺序，保证确定性。
 */

export interface SelectionInput {
  pool: Exercise[];
  count: number;
  targetDifficulty: number;
  /** exerciseId → 最近一次作答时间戳（毫秒） */
  seenAt: Map<string, number>;
  now: Date;
}

export function selectExercises(input: SelectionInput): Exercise[] {
  const { pool, count, targetDifficulty, seenAt, now } = input;
  const nowMs = now.getTime();
  const DAY = 86400000;

  const scored = pool.map((exercise, index) => {
    const lastSeen = seenAt.get(exercise.id);
    let recentPenalty = 0;
    if (lastSeen !== undefined) {
      const days = (nowMs - lastSeen) / DAY;
      if (days < 1) recentPenalty = 6;
      else if (days < 3) recentPenalty = 3;
      else if (days < 7) recentPenalty = 1;
    }
    const difficultyGap = Math.abs(exercise.difficulty - targetDifficulty);
    return { exercise, index, score: difficultyGap * 2 + recentPenalty };
  });

  return scored
    .sort((a, b) => (a.score === b.score ? a.index - b.index : a.score - b.score))
    .slice(0, Math.max(0, count))
    .map((item) => item.exercise);
}


