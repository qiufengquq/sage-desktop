import { Network } from "lucide-react";
import { cn } from "@/lib/utils";
import type { KnowledgeSummary } from "@/types";
import { DIFFICULTY_LABEL, MASTERY_STATE_LABEL } from "@/types/vocabulary";
import { KNOWLEDGE_KIND_LABEL } from "@/types/knowledge";
import { formatRelativeDay } from "@/lib/date";
import { Badge } from "@/components/ui/badge";
import { MasteryBar } from "./mastery-bar";

const KIND_VARIANT = {
  word: "brand",
  phrase: "ember",
  grammar: "success",
  sentence: "default",
} as const;

interface KnowledgeCardProps {
  item: KnowledgeSummary;
  active?: boolean;
  onClick?: (item: KnowledgeSummary) => void;
  className?: string;
  /** 关联数量，出现在卡片右侧 */
  degree?: number;
}

export function KnowledgeCard({ item, active, onClick, className, degree }: KnowledgeCardProps) {
  return (
    <button
      type="button"
      onClick={() => onClick?.(item)}
      className={cn(
        "group w-full rounded-md border px-3 py-2.5 text-left transition-colors duration-150",
        active
          ? "border-brand/45 bg-brand-tint/40"
          : "border-line bg-surface hover:border-line-strong hover:bg-raised",
        className,
      )}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5">
            <Badge variant={KIND_VARIANT[item.kind]}>{KNOWLEDGE_KIND_LABEL[item.kind]}</Badge>
            <span className="truncate text-2xs text-ink-muted">
              {DIFFICULTY_LABEL[item.difficulty]}
            </span>
          </div>
          <div className="mt-1 truncate text-[13.5px] font-medium text-ink">{item.primary}</div>
          <div className="mt-0.5 line-clamp-1 text-2xs text-ink-muted">{item.secondary}</div>
        </div>

        <div className="flex shrink-0 flex-col items-end gap-1">
          <span className="text-2xs text-ink-muted">{MASTERY_STATE_LABEL[item.state]}</span>
          {degree !== undefined && (
            <span className="inline-flex items-center gap-0.5 font-mono text-2xs text-ink-muted">
              <Network className="size-3" />
              {degree}
            </span>
          )}
        </div>
      </div>

      <div className="mt-2 flex items-center gap-3">
        <MasteryBar value={item.mastery} className="flex-1" />
        {item.dueAt && (
          <span className="shrink-0 text-2xs text-ink-muted">
            {formatRelativeDay(item.dueAt)}复习
          </span>
        )}
      </div>
    </button>
  );
}
