import type { EntityStateSeed, MasteryState } from "@/types";

import { grammarStateHints } from "./grammar";
import { sentenceStateHints } from "./sentences";
import { wordStateHints } from "./vocabulary";

/**
 * 种子学习态的统一索引。
 *
 * 为什么需要它：内容实体（`Vocabulary` / `GrammarPoint` / `Sentence`）已经从类型上
 * 剥离了 `mastery / state / dueAt / strength / lastReviewedAt`，掌握度只由
 * `db.entityStates` 提供。但 mock 的静态数据（知识图谱、示例统计）在建库之前
 * 也需要一组初始值——它们统一从这里取，而不是在多个文件里各自硬编码。
 *
 * 约束：这些值**只用于首次建库与静态演示**，任何运行时读取都必须走 `entityStates`。
 */
export const stateHintById: Map<string, EntityStateSeed> = new Map(
  [...wordStateHints, ...grammarStateHints, ...sentenceStateHints].map((hint) => [
    hint.entityId,
    hint,
  ]),
);

export function hintMastery(id: string): number {
  return stateHintById.get(id)?.mastery ?? 0;
}

export function hintState(id: string): MasteryState {
  return stateHintById.get(id)?.state ?? "new";
}

export function hintStrength(id: string): number {
  return stateHintById.get(id)?.strength ?? 0;
}

export function hintDueAt(id: string): string | null {
  return stateHintById.get(id)?.dueAt ?? null;
}

export function hintLastReviewedAt(id: string): string | null {
  return stateHintById.get(id)?.lastReviewedAt ?? null;
}

export function hintReviewCount(id: string): number {
  return stateHintById.get(id)?.reviewCount ?? 0;
}
