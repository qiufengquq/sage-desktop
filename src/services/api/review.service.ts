import type { ApiResult, RequestOptions, ReviewCard, ReviewRating } from "@/types";
import { latency } from "@/services/mock/base";
import { applyReviewSubmit } from "@/services/data/learning-db";
import { buildReviewCards, difficultyOf, dueItems } from "@/services/data/selectors";
import { commitDb, getDb } from "@/stores/learning-store";
import { COMMANDS } from "./contracts";
import { serve } from "./transport";

export interface ReviewQueuePayload {
  /** 当前所有到期条目数（可能大于 cards.length） */
  dueCount: number;
  cards: ReviewCard[];
}

export interface ReviewSubmitResult {
  entityId: string;
  rating: ReviewRating;
  masteryBefore: number;
  masteryAfter: number;
  nextDueAt: string;
  intervalDays: number;
  ease: number;
  xp: number;
  /** 本次调度的人类可读说明 */
  explanation: string;
}

const RATING_EXPLANATION: Record<ReviewRating, string> = {
  again: "标记为忘记：回到重学阶段，10 分钟后再出现，掌握度明显下调。",
  hard: "标记为模糊：间隔小幅延长，掌握度小幅提升。",
  good: "标记为认识：间隔按难度因子延长，掌握度提升。",
  easy: "标记为很熟：间隔延长更多，掌握度提升最多。",
};

/** 领取当前到期的复习卡片（按逾期时间从早到晚） */
export function getReviewQueue(
  limit = 20,
  options: RequestOptions = {},
): Promise<ApiResult<ReviewQueuePayload>> {
  return serve(
    COMMANDS.reviewQueue,
    async () => {
      await latency(options.signal);
      const now = new Date();
      const db = getDb(now);
      return {
        dueCount: dueItems(db, now).length,
        cards: buildReviewCards(db, now, limit),
      };
    },
    { args: { limit }, request: options },
  );
}

/**
 * 提交一次自评：更新掌握度与下一次复习时间，并写入复习日志。
 *
 * `submissionId` 是幂等键（与 `assessment_submit` 同一约定）：写到 `review_logs.submission_id`
 * 的唯一索引上，网络重试或双击不会重复排期、重复发 XP。mock 分支不使用它。
 */
export function submitReview(input: {
  entityId: string;
  rating: ReviewRating;
}): Promise<ApiResult<ReviewSubmitResult>> {
  const submissionId = crypto.randomUUID();
  return serve(
    COMMANDS.reviewSubmit,
    async () => {
      await latency();
      const now = new Date();
      const difficulty = difficultyOf(input.entityId);
      let result: ReviewSubmitResult | null = null;

      commitDb((db) => {
        const applied = applyReviewSubmit(db, {
          entityId: input.entityId,
          rating: input.rating,
          difficulty,
          now,
        });
        result = {
          entityId: input.entityId,
          rating: input.rating,
          masteryBefore: applied.log.masteryBefore,
          masteryAfter: applied.log.masteryAfter,
          nextDueAt: applied.log.nextDueAt,
          intervalDays: applied.log.nextIntervalDays,
          ease: applied.log.ease,
          xp: applied.log.xp,
          explanation: RATING_EXPLANATION[input.rating],
        };
        return applied.db;
      });

      if (!result) throw new Error("internal:复习提交失败");
      return result;
    },
    { args: { ...input, submissionId } },
  );
}
