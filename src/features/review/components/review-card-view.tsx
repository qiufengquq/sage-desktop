import { Eye, Layers } from "lucide-react";
import type { ReviewCard, ReviewRating } from "@/types";
import { REVIEW_RATING_LABEL } from "@/types/review";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { MasteryBar } from "@/components/common/mastery-bar";

const RATING_ORDER: ReviewRating[] = ["again", "hard", "good", "easy"];

const RATING_STYLE: Record<ReviewRating, string> = {
  again: "border-danger/30 text-danger hover:bg-danger/[0.06]",
  hard: "border-warning/35 text-warning hover:bg-warning/[0.07]",
  good: "border-brand/30 text-brand hover:bg-brand-tint",
  easy: "border-success/30 text-success hover:bg-success/[0.06]",
};

export function ReviewCardView({
  card,
  index,
  total,
  revealed,
  busy,
  onReveal,
  onRate,
}: {
  card: ReviewCard;
  index: number;
  total: number;
  revealed: boolean;
  busy: boolean;
  onReveal: () => void;
  onRate: (rating: ReviewRating) => void;
}) {
  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <span className="font-mono text-2xs text-ink-muted">
          {index + 1} / {total}
        </span>
        <div className="flex items-center gap-2">
          <Badge variant="outline" className="font-mono">
            {card.item.kind === "grammar" ? "语法" : card.item.kind === "sentence" ? "句子" : "词汇"}
          </Badge>
          {card.item.overdueDays > 0 && (
            <Badge variant="warning">逾期 {card.item.overdueDays} 天</Badge>
          )}
        </div>
      </div>

      <div className="rounded-md border border-line bg-surface p-6">
        <p className="text-2xs text-ink-muted">{revealed ? "答案" : "先在心里回忆，再揭晓"}</p>
        <div className="mt-2 font-display text-[26px] leading-snug text-ink">{card.prompt}</div>
        <p className="mt-1 text-2xs text-ink-muted">{card.focus}</p>

        {revealed ? (
          <div className="mt-5 space-y-3 border-t border-line pt-4">
            <p className="whitespace-pre-line text-[15px] leading-relaxed text-ink">{card.answer}</p>
            {card.detail.length > 0 && (
              <ul className="space-y-1.5">
                {card.detail.map((line) => (
                  <li key={line} className="text-2xs leading-relaxed text-ink-soft">
                    {line}
                  </li>
                ))}
              </ul>
            )}
            <div className="pt-2">
              <div className="mb-1 flex items-center justify-between text-2xs text-ink-muted">
                <span>当前掌握度</span>
                <span className="font-mono">{Math.round(card.item.mastery)} 分</span>
              </div>
              <MasteryBar value={card.item.mastery} showValue={false} />
            </div>
          </div>
        ) : (
          <Button className="mt-5" variant="outline" onClick={onReveal}>
            <Eye />
            显示答案
          </Button>
        )}
      </div>

      {revealed && (
        <div>
          <p className="mb-2 text-2xs text-ink-muted">
            刚才回忆得怎么样？选择后决定下次什么时候再出现。
          </p>
          <div className="grid gap-2 sm:grid-cols-4">
            {RATING_ORDER.map((rating) => (
              <button
                key={rating}
                type="button"
                disabled={busy}
                onClick={() => onRate(rating)}
                className={cn(
                  "rounded-md border bg-surface px-3 py-2.5 text-left transition-colors disabled:opacity-60",
                  RATING_STYLE[rating],
                )}
              >
                <span className="block text-[13px] font-medium">
                  {REVIEW_RATING_LABEL[rating]}
                </span>
                <span className="mt-0.5 block font-mono text-2xs text-ink-muted">
                  {card.preview[rating]}
                </span>
              </button>
            ))}
          </div>
        </div>
      )}

      <p className="flex items-center gap-1.5 text-2xs text-ink-muted">
        <Layers className="size-3" />
        已复习 {card.item.reviewCount} 次 · 记忆强度 {Math.round(card.item.strength * 100)}%
      </p>
    </div>
  );
}
