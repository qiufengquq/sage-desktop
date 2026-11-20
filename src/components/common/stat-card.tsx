import type { LucideIcon } from "lucide-react";
import { ArrowDownRight, ArrowUpRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { Sparkline } from "@/components/charts/sparkline";

interface StatCardProps {
  label: string;
  value: string;
  unit?: string;
  delta?: number;
  /** delta 的语义，决定颜色与方向说明 */
  deltaLabel?: string;
  hint?: string;
  icon?: LucideIcon;
  trend?: number[];
  variant?: "card" | "bare";
  className?: string;
}

export function StatCard({
  label,
  value,
  unit,
  delta,
  deltaLabel,
  hint,
  icon: Icon,
  trend,
  variant = "card",
  className,
}: StatCardProps) {
  const positive = (delta ?? 0) >= 0;

  return (
    <div
      className={cn(
        "flex flex-col justify-between gap-2",
        variant === "card" && "rounded-md border border-line bg-surface p-4",
        variant === "bare" && "px-4 py-1",
        className,
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="text-2xs font-medium tracking-wide text-ink-muted">{label}</span>
        {Icon && <Icon className="size-3.5 text-ink-muted/70" />}
      </div>

      <div className="flex items-baseline gap-1">
        <span className="type-numeral text-[22px] leading-none text-ink">{value}</span>
        {unit && <span className="text-2xs text-ink-muted">{unit}</span>}
      </div>

      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-1.5">
          {delta !== undefined && (
            <span
              className={cn(
                "inline-flex items-center gap-0.5 font-mono text-2xs",
                positive ? "text-success" : "text-danger",
              )}
            >
              {positive ? (
                <ArrowUpRight className="size-3" />
              ) : (
                <ArrowDownRight className="size-3" />
              )}
              {Math.abs(delta)}
              {deltaLabel}
            </span>
          )}
          {hint && <span className="text-2xs text-ink-muted">{hint}</span>}
        </div>
        {trend && trend.length > 1 && (
          <Sparkline values={trend} height={18} className="max-w-16 opacity-80" />
        )}
      </div>
    </div>
  );
}
