import { cn } from "@/lib/utils";

interface RadialGaugeProps {
  value: number;
  label?: string;
  size?: number;
  thickness?: number;
  className?: string;
  color?: string;
  /** 参考线，例如同水平均值 */
  benchmark?: number;
}

/** 环形进度：用于掌握度与能力评分 */
export function RadialGauge({
  value,
  label,
  size = 96,
  thickness = 6,
  className,
  color = "var(--brand)",
  benchmark,
}: RadialGaugeProps) {
  const radius = (size - thickness) / 2;
  const circumference = 2 * Math.PI * radius;
  const clamped = Math.min(100, Math.max(0, value));
  const dash = (clamped / 100) * circumference;
  const benchDash = benchmark === undefined ? 0 : (Math.min(100, benchmark) / 100) * circumference;

  return (
    <div className={cn("relative inline-flex items-center justify-center", className)}>
      <svg width={size} height={size} className="-rotate-90" role="img" aria-label={`${label ?? "进度"} ${clamped}%`}>
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          fill="none"
          stroke="var(--sunken)"
          strokeWidth={thickness}
        />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          fill="none"
          stroke={color}
          strokeWidth={thickness}
          strokeLinecap="round"
          strokeDasharray={`${dash} ${circumference - dash}`}
          className="transition-[stroke-dasharray] duration-700 ease-instrument"
        />
        {benchmark !== undefined && (
          <circle
            cx={size / 2}
            cy={size / 2}
            r={radius}
            fill="none"
            stroke="var(--ink-muted)"
            strokeWidth={1}
            strokeDasharray={`1 ${circumference / 60}`}
            strokeDashoffset={-benchDash}
            opacity={0.55}
          />
        )}
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center">
        <span className="type-numeral text-lg leading-none text-ink">{Math.round(clamped)}</span>
        {label && <span className="mt-1 text-[10px] text-ink-muted">{label}</span>}
      </div>
    </div>
  );
}
