import { ArrowRight, CheckCircle2, RotateCcw } from "lucide-react";
import type { ReviewSubmitResult } from "@/services/api/review.service";
import type { ReviewRating } from "@/types";
import { REVIEW_RATING_LABEL } from "@/types/review";
import { formatDuration } from "@/lib/format";
import { formatRelativeTime } from "@/lib/date";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { RadialGauge } from "@/components/charts/radial-gauge";

const RATING_ORDER: ReviewRating[] = ["again", "hard", "good", "easy"];

export function ReviewSummaryView({
  results,
  remaining,
  nextDueAt,
  durationMs,
  onAgain,
  onHome,
  onAnalytics,
}: {
  results: ReviewSubmitResult[];
  remaining: number;
  nextDueAt: string | null;
  durationMs: number;
  onAgain: () => void;
  onHome: () => void;
  onAnalytics: () => void;
}) {
  const total = results.length;
  const counts = RATING_ORDER.map((rating) => ({
    rating,
    count: results.filter((result) => result.rating === rating).length,
  }));
  const xp = results.reduce((acc, result) => acc + result.xp, 0);
  const remembered = results.filter((result) => result.rating === "good" || result.rating === "easy")
    .length;
  const rate = total === 0 ? 0 : Math.round((remembered / total) * 100);

  return (
    <div className="space-y-5">
      <div className="rounded-md border border-line bg-surface p-5">
        <div className="flex flex-wrap items-center gap-5">
          <RadialGauge value={rate} size={80} thickness={6} label="记住率" />
          <div className="min-w-0 flex-1">
            <h2 className="flex items-center gap-2 font-display text-[22px] leading-tight text-ink">
              <CheckCircle2 className="size-4 text-success" />
              本轮复习完成
            </h2>
            <p className="mt-1 max-w-[54ch] text-2xs leading-relaxed text-ink-soft">
              共复习 {total} 项，用时 {formatDuration(durationMs)}，获得 {xp} XP。
              {nextDueAt
                ? `下一次到期在 ${formatRelativeTime(nextDueAt)}。`
                : "当前没有新的到期条目。"}
            </p>
          </div>
        </div>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <section className="rounded-md border border-line bg-surface p-4">
          <h3 className="text-[13.5px] font-medium text-ink">回忆分布</h3>
          <div className="mt-3 space-y-2">
            {counts.map((item) => (
              <div key={item.rating} className="flex items-center justify-between text-2xs">
                <span className="text-ink-soft">{REVIEW_RATING_LABEL[item.rating]}</span>
                <div className="flex items-center gap-2">
                  <div className="h-1.5 w-28 overflow-hidden rounded-full bg-sunken">
                    <span
                      className="block h-full rounded-full bg-brand"
                      style={{ width: `${total === 0 ? 0 : (item.count / total) * 100}%` }}
                    />
                  </div>
                  <span className="w-6 text-right font-mono text-ink-muted">{item.count}</span>
                </div>
              </div>
            ))}
          </div>
        </section>

        <section className="rounded-md border border-line bg-surface p-4">
          <h3 className="text-[13.5px] font-medium text-ink">调度结果</h3>
          <p className="mt-1 text-2xs text-ink-muted">
            每个条目的下一次复习时间已按自评重新计算，掌握度也同步更新到知识库。
          </p>
          <ul className="mt-3 space-y-1.5">
            {results.slice(0, 4).map((result) => (
              <li
                key={`${result.entityId}-${result.nextDueAt}`}
                className="flex items-center justify-between gap-2 text-2xs"
              >
                <span className="truncate text-ink-soft">{result.explanation}</span>
                <Badge variant="outline" className="shrink-0 font-mono">
                  {result.intervalDays === 0 ? "10 分钟" : `${result.intervalDays} 天`}
                </Badge>
              </li>
            ))}
          </ul>
          <p className="mt-3 text-2xs text-ink-muted">
            剩余到期：<span className="font-mono text-ink-soft">{remaining}</span> 项
          </p>
        </section>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" onClick={onAgain} disabled={remaining === 0}>
          <RotateCcw />
          再复习一轮
        </Button>
        <Button onClick={onHome}>
          返回首页
          <ArrowRight />
        </Button>
        <Button variant="ghost" onClick={onAnalytics} className="text-ink-muted">
          查看学习数据
        </Button>
      </div>
    </div>
  );
}
