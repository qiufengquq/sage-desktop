import { cn } from "@/lib/utils";

interface MasteryBarProps {
  value: number;
  label?: string;
  className?: string;
  showValue?: boolean;
  /** 记忆强度不足时用警示色，表达"正在遗忘" */
  tone?: "auto" | "brand" | "ember" | "danger";
}

export function MasteryBar({
  value,
  label,
  className,
  showValue = true,
  tone = "auto",
}: MasteryBarProps) {
  const clamped = Math.min(100, Math.max(0, Math.round(value)));
  const color =
    tone === "auto"
      ? clamped >= 80
        ? "var(--success)"
        : clamped >= 50
          ? "var(--brand)"
          : clamped >= 30
            ? "var(--ember)"
            : "var(--danger)"
      : tone === "brand"
        ? "var(--brand)"
        : tone === "ember"
          ? "var(--ember)"
          : "var(--danger)";

  return (
    <div className={cn("flex items-center gap-2", className)}>
      {label && <span className="w-14 shrink-0 text-2xs text-ink-muted">{label}</span>}
      <div className="relative h-1 flex-1 overflow-hidden rounded-full bg-sunken">
        <span
          className="absolute inset-y-0 left-0 rounded-full transition-[width] duration-500 ease-instrument"
          style={{ width: `${clamped}%`, backgroundColor: color }}
        />
      </div>
      {showValue && (
        <span className="w-7 shrink-0 text-right font-mono text-2xs text-ink-muted tabular-nums">
          {clamped}
        </span>
      )}
    </div>
  );
}
