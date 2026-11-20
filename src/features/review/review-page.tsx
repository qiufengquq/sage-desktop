import { useCallback, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { ArrowRight, CalendarClock, Loader2, RefreshCw } from "lucide-react";
import type { ApiError, ReviewCard, ReviewRating } from "@/types";
import { REVIEW_RATING_LABEL } from "@/types/review";
import { reviewService, type ReviewSubmitResult } from "@/services/api";
import { useQuery } from "@/hooks/use-query";
import { formatDuration } from "@/lib/format";
import { PageContainer } from "@/components/common/page-container";
import { PageHeader } from "@/components/common/page-header";
import { ErrorState, LoadingState } from "@/components/common/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ReviewCardView } from "./components/review-card-view";
import { ReviewSummaryView } from "./components/review-summary-view";

type Stage = "idle" | "running" | "summary";

export function ReviewPage() {
  const navigate = useNavigate();
  const { data, error, reload, initial } = useQuery(
    (options) => reviewService.getReviewQueue(20, options),
    [],
  );

  const [stage, setStage] = useState<Stage>("idle");
  const [cards, setCards] = useState<ReviewCard[]>([]);
  const [index, setIndex] = useState(0);
  const [revealed, setRevealed] = useState(false);
  const [results, setResults] = useState<ReviewSubmitResult[]>([]);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<ApiError | null>(null);
  const [remaining, setRemaining] = useState(0);
  const [nextDueAt, setNextDueAt] = useState<string | null>(null);
  const [durationMs, setDurationMs] = useState(0);
  const startedAt = useRef(0);

  const begin = useCallback(
    (queue: ReviewCard[]) => {
      setCards(queue);
      setIndex(0);
      setRevealed(false);
      setResults([]);
      setActionError(null);
      startedAt.current = Date.now();
      setStage("running");
    },
    [],
  );

  const finish = useCallback(async (collected: ReviewSubmitResult[]) => {
    setDurationMs(Date.now() - startedAt.current);
    setStage("summary");
    const queue = await reviewService.getReviewQueue(1);
    if (queue.ok) {
      setRemaining(queue.data.dueCount);
      setNextDueAt(queue.data.cards[0]?.item.dueAt ?? null);
    }
    setResults(collected);
  }, []);

  const rate = async (rating: ReviewRating) => {
    const card = cards[index];
    if (!card || busy) return;
    setBusy(true);
    const response = await reviewService.submitReview({
      entityId: card.item.entityId,
      rating,
    });
    setBusy(false);

    if (!response.ok) {
      setActionError(response.error);
      return;
    }

    const collected = [...results, response.data];
    setResults(collected);

    if (index + 1 >= cards.length) {
      await finish(collected);
      return;
    }
    setIndex((current) => current + 1);
    setRevealed(false);
  };

  if (stage === "running" && cards[index]) {
    return (
      <PageContainer className="max-w-[820px]">
        <div className="mb-3 flex items-center justify-between">
          <div className="text-[13px] font-medium text-ink">复习</div>
          <Button
            variant="ghost"
            size="sm"
            className="text-ink-muted"
            onClick={() => finish(results)}
          >
            结束本轮
          </Button>
        </div>
        <ReviewCardView
          card={cards[index]}
          index={index}
          total={cards.length}
          revealed={revealed}
          busy={busy}
          onReveal={() => setRevealed(true)}
          onRate={rate}
        />
        {actionError && <ErrorState error={actionError} className="mt-4" />}
      </PageContainer>
    );
  }

  if (stage === "summary") {
    return (
      <PageContainer className="max-w-[900px]">
        <ReviewSummaryView
          results={results}
          remaining={remaining}
          nextDueAt={nextDueAt}
          durationMs={durationMs}
          onAgain={() => {
            // 上一轮的卡片已经重新排期，必须重新取队列，不能复用旧快照
            setResults([]);
            setStage("idle");
            reload();
          }}
          onHome={() => navigate("/")}
          onAnalytics={() => navigate("/analytics")}
        />
      </PageContainer>
    );
  }

  const dueCount = data?.dueCount ?? 0;
  const preview = data?.cards.slice(0, 5) ?? [];

  return (
    <PageContainer className="max-w-[900px]">
      <PageHeader
        title="复习"
        description="按遗忘曲线排出的到期条目。先回忆，再揭晓，然后按真实感受自评——下一次出现的时间由你的自评决定。"
        meta={
          <>
            <span className="flex items-center gap-1 text-2xs text-ink-muted">
              <CalendarClock className="size-3" />
              到期 {dueCount} 项
            </span>
            <span className="text-2xs text-ink-muted">单轮约 {Math.max(1, Math.round(dueCount * 0.4))} 分钟</span>
          </>
        }
        actions={
          <Button variant="ghost" size="sm" onClick={reload} className="text-ink-muted">
            <RefreshCw />
            刷新
          </Button>
        }
      />

      {initial && <LoadingState variant="cards" rows={3} />}
      {error && <ErrorState error={error} onRetry={reload} />}

      {data && (
        <div className="space-y-4">
          <div className="rounded-md border border-line bg-surface p-5">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <h2 className="font-display text-[20px] leading-tight text-ink">
                  {dueCount === 0 ? "今天没有到期条目" : `${dueCount} 项等待复习`}
                </h2>
                <p className="mt-1 max-w-[56ch] text-2xs leading-relaxed text-ink-soft">
                  {dueCount === 0
                    ? "复习队列已经清空。可以去做一组训练，或者明天再来。"
                    : "每轮最多 20 项。复习结果会更新掌握度、下一次复习时间与今日学习统计。"}
                </p>
              </div>
              <Button
                disabled={dueCount === 0}
                onClick={() => data.cards.length > 0 && begin(data.cards)}
              >
                开始复习
                <ArrowRight />
              </Button>
            </div>
          </div>

          {preview.length > 0 && (
            <section className="rounded-md border border-line bg-surface p-4">
              <h3 className="text-[13.5px] font-medium text-ink">本轮将会出现</h3>
              <ul className="mt-3 divide-y divide-line">
                {preview.map((card) => (
                  <li key={card.item.entityId} className="flex items-center justify-between gap-3 py-2">
                    <div className="min-w-0">
                      <div className="truncate text-[13px] text-ink">{card.item.label}</div>
                      <div className="truncate text-2xs text-ink-muted">{card.item.secondary}</div>
                    </div>
                    <div className="flex shrink-0 items-center gap-2">
                      {card.item.overdueDays > 0 && (
                        <Badge variant="warning" className="font-mono">
                          逾期 {card.item.overdueDays} 天
                        </Badge>
                      )}
                      <span className="font-mono text-2xs text-ink-muted">
                        {Math.round(card.item.mastery)} 分
                      </span>
                    </div>
                  </li>
                ))}
              </ul>
              {dueCount > preview.length && (
                <p className="mt-2 text-2xs text-ink-muted">
                  还有 {dueCount - preview.length} 项不在预览中。
                </p>
              )}
            </section>
          )}

          <section className="rounded-md border border-line bg-raised/60 p-4">
            <h3 className="text-[13.5px] font-medium text-ink">自评如何影响后续</h3>
            <ul className="mt-2 space-y-1.5 text-2xs leading-relaxed text-ink-soft">
              {(["again", "hard", "good", "easy"] as ReviewRating[]).map((rating) => (
                <li key={rating}>
                  <span className="text-ink">{REVIEW_RATING_LABEL[rating]}</span>
                  {" · "}
                  {rating === "again"
                    ? "10 分钟后重新出现，掌握度明显下调"
                    : rating === "hard"
                      ? "间隔小幅延长，掌握度小幅提升"
                      : rating === "good"
                        ? "按难度因子延长间隔"
                        : "间隔延长更多，掌握度提升最多"}
                </li>
              ))}
            </ul>
            <p className="mt-2 flex items-center gap-1.5 text-2xs text-ink-muted">
              {busy && <Loader2 className="size-3 animate-spin" />}
              调度由 SM-2 变体计算，不使用随机数，可在学习算法文档中逐条核对。
            </p>
          </section>
        </div>
      )}

      {results.length > 0 && stage === "idle" && (
        <p className="mt-4 text-2xs text-ink-muted">
          上一轮复习了 {results.length} 项，用时 {formatDuration(durationMs)}。
        </p>
      )}
    </PageContainer>
  );
}
