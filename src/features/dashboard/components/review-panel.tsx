import { ArrowRight, CalendarClock, Lightbulb } from "lucide-react";
import type { DashboardData } from "@/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { MasteryBar } from "@/components/common/mastery-bar";
import { SectionHeader } from "@/components/common/page-header";

export function ReviewPanel({
  data,
  onOpenReview,
  onOpenKnowledge,
}: {
  data: DashboardData;
  onOpenReview: () => void;
  onOpenKnowledge: (id: string) => void;
}) {
  const overdue = data.reviewTasks.filter((r) => r.overdueDays > 0).length;

  return (
    <section>
      <SectionHeader
        title="今日复习"
        description={
          overdue > 0
            ? `${data.reviewTasks.length} 项到期，其中 ${overdue} 项已逾期`
            : `${data.reviewTasks.length} 项到期`
        }
        action={
          <Button size="sm" variant="ghost" onClick={onOpenReview}>
            全部复习
            <ArrowRight />
          </Button>
        }
      />
      <div className="divide-y divide-line rounded-md border border-line bg-surface">
        {data.reviewTasks.map((item) => (
          <button
            key={item.id}
            type="button"
            onClick={() => onOpenKnowledge(item.entityId)}
            className="flex w-full items-center gap-3 px-4 py-2.5 text-left transition-colors hover:bg-raised"
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="truncate text-[13px] text-ink">{item.entityLabel}</span>
                {item.overdueDays > 0 && (
                  <Badge variant="danger">逾期 {item.overdueDays} 天</Badge>
                )}
              </div>
              <div className="mt-1.5 flex items-center gap-2">
                <MasteryBar value={item.strength * 100} showValue={false} className="w-20" />
                <span className="text-2xs text-ink-muted">
                  记忆强度 {(item.strength * 100).toFixed(0)}%
                </span>
              </div>
            </div>
            <CalendarClock className="size-3.5 shrink-0 text-ink-muted/60" />
          </button>
        ))}
      </div>
    </section>
  );
}

export function AiNudge({
  data,
  onApply,
}: {
  data: DashboardData;
  onApply: () => void;
}) {
  const advice = data.recommendation;
  if (!advice) return null;

  return (
    <section className="rounded-md border border-brand/25 bg-brand-tint/35 p-4">
      <div className="flex items-center gap-1.5">
        <Lightbulb className="size-3.5 text-brand" />
        <span className="text-2xs font-medium text-brand">学习建议 · 基于最近 30 天数据</span>
      </div>
      <h3 className="mt-2 font-display text-[16px] leading-snug text-ink">{advice.title}</h3>
      <p className="mt-1.5 text-[12.5px] leading-relaxed text-ink-soft">{advice.body}</p>
      <ul className="mt-2.5 space-y-1">
        {advice.evidence.map((line) => (
          <li key={line} className="flex gap-1.5 text-2xs text-ink-muted">
            <span className="mt-1.5 size-1 shrink-0 rounded-full bg-brand/60" aria-hidden />
            {line}
          </li>
        ))}
      </ul>
      <Button size="sm" variant="subtle" className="mt-3" onClick={onApply}>
        {advice.action.label}
        <ArrowRight />
      </Button>
    </section>
  );
}
