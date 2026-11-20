import { Check, ChevronRight, Loader2, Play } from "lucide-react";
import { cn } from "@/lib/utils";
import type { DashboardTask } from "@/types";
import { ACTIVITY_LABEL } from "@/types/learning";
import { planActionLabel } from "@/lib/plan-target";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

interface TaskCardProps {
  task: DashboardTask;
  onStart?: (task: DashboardTask) => void;
  /** 时间轴样式：左侧显示顺序索引与连接线 */
  index?: number;
  isLast?: boolean;
  className?: string;
}

const STATUS_META = {
  done: { label: "已完成", variant: "success" as const, Icon: Check },
  "in-progress": { label: "进行中", variant: "brand" as const, Icon: Loader2 },
  pending: { label: "待开始", variant: "outline" as const, Icon: Play },
  skipped: { label: "已跳过", variant: "default" as const, Icon: ChevronRight },
};

export function TaskCard({ task, onStart, index, isLast = false, className }: TaskCardProps) {
  const meta = STATUS_META[task.status];
  const StatusIcon = meta.Icon;
  const done = task.status === "done";

  return (
    <div className={cn("group relative flex gap-4", className)}>
      {index !== undefined && (
        <div className="relative flex w-7 shrink-0 flex-col items-center pt-1">
          <span
            className={cn(
              "flex size-6 items-center justify-center rounded-full font-mono text-2xs transition-colors",
              done
                ? "bg-brand text-brand-ink"
                : task.status === "in-progress"
                  ? "bg-brand-tint text-brand ring-1 ring-brand/40 ring-inset"
                  : "bg-sunken text-ink-muted",
            )}
          >
            {done ? <Check className="size-3" strokeWidth={3} /> : index + 1}
          </span>
          {!isLast && <span className="mt-1 w-px flex-1 bg-line" aria-hidden />}
        </div>
      )}

      <div className={cn("min-w-0 flex-1", !isLast && "pb-5")}>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h3
                className={cn(
                  "text-[13.5px] font-medium",
                  done ? "text-ink-muted line-through decoration-line-strong" : "text-ink",
                )}
              >
                {task.title}
              </h3>
              <Badge variant={meta.variant}>
                <StatusIcon className={cn("size-3", task.status === "in-progress" && "animate-spin")} />
                {meta.label}
              </Badge>
            </div>
            <p className="mt-1 max-w-[58ch] text-2xs leading-relaxed text-ink-muted">
              {task.reason}
            </p>
          </div>

          <div className="flex shrink-0 items-center gap-2">
            <span className="font-mono text-2xs text-ink-muted">{task.minutes} 分钟</span>
            <span className="text-ink-muted/40">·</span>
            <span className="font-mono text-2xs text-ink-muted">+{task.xp} XP</span>
            <Button
              size="sm"
              variant={done ? "ghost" : "outline"}
              onClick={() => onStart?.(task)}
              className="ml-1"
            >
              {done ? "重练" : "开始"}
            </Button>
          </div>
        </div>

        <div className="mt-1.5 flex items-center gap-2 text-2xs text-ink-muted">
          <span>{ACTIVITY_LABEL[task.activity]}</span>
          <span className="text-ink-muted/40">·</span>
          <span className="font-mono">{planActionLabel(task.action)}</span>
        </div>
      </div>
    </div>
  );
}
