import type { LucideIcon } from "lucide-react";
import { Inbox, RotateCcw, TriangleAlert } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ApiError } from "@/types";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";

/* ---------------------------------- 空状态 --------------------------------- */
interface EmptyStateProps {
  title: string;
  description?: string;
  icon?: LucideIcon;
  action?: React.ReactNode;
  className?: string;
}

export function EmptyState({
  title,
  description,
  icon: Icon = Inbox,
  action,
  className,
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center rounded-md border border-dashed border-line px-6 py-12 text-center",
        className,
      )}
    >
      <span className="flex size-9 items-center justify-center rounded-full bg-sunken text-ink-muted">
        <Icon className="size-4" />
      </span>
      <h3 className="mt-3 text-[13.5px] font-medium text-ink">{title}</h3>
      {description && (
        <p className="mt-1 max-w-[46ch] text-2xs leading-relaxed text-ink-muted">{description}</p>
      )}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}

/* --------------------------------- 加载状态 -------------------------------- */
interface LoadingStateProps {
  variant?: "list" | "cards" | "chart" | "text";
  rows?: number;
  className?: string;
  label?: string;
}

export function LoadingState({
  variant = "list",
  rows = 4,
  className,
  label = "正在读取数据",
}: LoadingStateProps) {
  if (variant === "chart") {
    return (
      <div className={cn("rounded-md border border-line bg-surface p-4", className)} role="status">
        <span className="sr-only">{label}</span>
        <Skeleton className="h-3 w-24" />
        <Skeleton className="mt-4 h-32 w-full" />
      </div>
    );
  }

  if (variant === "cards") {
    return (
      <div className={cn("grid gap-3 sm:grid-cols-2 lg:grid-cols-3", className)} role="status">
        <span className="sr-only">{label}</span>
        {Array.from({ length: rows }, (_, i) => (
          <div key={i} className="rounded-md border border-line bg-surface p-4">
            <Skeleton className="h-7 w-7 rounded-sm" />
            <Skeleton className="mt-3 h-3 w-20" />
            <Skeleton className="mt-2 h-3 w-full" />
            <Skeleton className="mt-1.5 h-3 w-2/3" />
          </div>
        ))}
      </div>
    );
  }

  if (variant === "text") {
    return (
      <div className={cn("space-y-2", className)} role="status">
        <span className="sr-only">{label}</span>
        <Skeleton className="h-3 w-1/3" />
        <Skeleton className="h-3 w-full" />
        <Skeleton className="h-3 w-4/5" />
      </div>
    );
  }

  return (
    <div className={cn("divide-y divide-line", className)} role="status">
      <span className="sr-only">{label}</span>
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="flex items-center gap-3 py-3">
          <Skeleton className="size-6 rounded-full" />
          <div className="flex-1 space-y-1.5">
            <Skeleton className="h-3 w-1/3" />
            <Skeleton className="h-2.5 w-2/3" />
          </div>
          <Skeleton className="h-6 w-14 rounded-sm" />
        </div>
      ))}
    </div>
  );
}

/* --------------------------------- 错误状态 -------------------------------- */
export function ErrorState({
  error,
  onRetry,
  className,
}: {
  error: ApiError;
  onRetry?: () => void;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col items-start gap-3 rounded-md border border-danger/25 bg-danger/[0.04] px-4 py-4",
        className,
      )}
      role="alert"
    >
      <div className="flex items-start gap-2.5">
        <TriangleAlert className="mt-0.5 size-4 shrink-0 text-danger" />
        <div>
          <p className="text-[13px] font-medium text-ink">{error.message}</p>
          {error.hint && <p className="mt-0.5 text-2xs text-ink-muted">{error.hint}</p>}
          {error.detail && (
            <p className="mt-1 font-mono text-2xs break-all text-ink-muted/80">{error.detail}</p>
          )}
        </div>
      </div>
      {onRetry && error.retryable !== false && (
        <Button size="sm" variant="outline" onClick={onRetry}>
          <RotateCcw />
          重试
        </Button>
      )}
    </div>
  );
}
