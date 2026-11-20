import type { ApiResult, PlanItem, RequestOptions, StudyPlan } from "@/types";
import { latency } from "@/services/mock/base";
import { decoratePlan } from "@/services/data/selectors";
import {
  applyDifficulty as applyDifficultyToDb,
  applyPlanToggle,
  rebuildPlan,
} from "@/services/data/learning-db";
import { commitDb, getDb } from "@/stores/learning-store";
import { COMMANDS } from "./contracts";
import { serve } from "./transport";

function ensurePlan(now: Date): StudyPlan {
  const db = getDb(now);
  if (db.plan) return decoratePlan(db, now) as StudyPlan;
  const next = commitDb((current) => rebuildPlan(current, { reason: "scheduled", now }));
  return decoratePlan(next, now) as StudyPlan;
}

export function getPlan(options: RequestOptions = {}): Promise<ApiResult<StudyPlan>> {
  return serve(
    COMMANDS.planGet,
    async () => {
      await latency(options.signal);
      return ensurePlan(new Date());
    },
    { request: options },
  );
}

/** 重新生成：按当前画像、到期数量、正确率与每日预算重新计算，不是复制模板 */
export function regeneratePlan(options: RequestOptions = {}): Promise<ApiResult<StudyPlan>> {
  return serve(
    COMMANDS.planRegenerate,
    async () => {
      await latency(options.signal);
      const now = new Date();
      const next = commitDb((db) => rebuildPlan(db, { reason: "regenerate", now }));
      return decoratePlan(next, now) as StudyPlan;
    },
    { request: options },
  );
}

export function applyDifficulty(level: number): Promise<ApiResult<StudyPlan>> {
  return serve(
    COMMANDS.planApplyDifficulty,
    async () => {
      await latency();
      const now = new Date();
      const next = commitDb((db) => applyDifficultyToDb(db, level));
      return decoratePlan(next, now) as StudyPlan;
    },
    { args: { level } },
  );
}

export function togglePlanItem(itemId: string): Promise<ApiResult<PlanItem[]>> {
  return serve(
    COMMANDS.planToggleItem,
    async () => {
      await latency();
      const next = commitDb((db) => applyPlanToggle(db, itemId));
      return next.plan?.today.items ?? [];
    },
    { args: { itemId } },
  );
}
