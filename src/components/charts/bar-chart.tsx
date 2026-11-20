import { useState } from "react";
import { cn } from "@/lib/utils";

export interface BarDatum {
  id: string;
  label: string;
  value: number;
  /** 可选的第二维度，用不同透明度的同色块表达 */
  secondary?: number;
  hint?: string;
}

interface BarChartProps {
  data: BarDatum[];
  height?: number;
  unit?: string;
  className?: string;
  color?: string;
  secondaryColor?: string;
}

export function BarChart({
  data,
  height = 160,
  unit = "",
  className,
  color = "var(--brand)",
  secondaryColor = "var(--ember)",
}: BarChartProps) {
  const [active, setActive] = useState<string | null>(null);
  const max = Math.max(...data.map((d) => d.value), 1);

  return (
    <div className={cn("w-full", className)}>
      <div className="flex items-end gap-1.5" style={{ height }}>
        {data.map((item) => {
          const h = Math.max(2, (item.value / max) * (height - 26));
          const isActive = active === item.id;
          return (
            <button
              key={item.id}
              type="button"
              className="group relative flex flex-1 cursor-default flex-col items-center justify-end gap-1"
              onMouseEnter={() => setActive(item.id)}
              onMouseLeave={() => setActive(null)}
              onFocus={() => setActive(item.id)}
              onBlur={() => setActive(null)}
              aria-label={`${item.label}：${item.value}${unit}`}
            >
              {isActive && (
                <span className="absolute -top-1 z-10 rounded-sm bg-ink px-1.5 py-0.5 font-mono text-2xs whitespace-nowrap text-paper">
                  {item.value}
                  {unit}
                </span>
              )}
              <span
                className="w-full rounded-t-[2px] transition-opacity"
                style={{
                  height: h,
                  backgroundColor: isActive ? color : `color-mix(in oklab, ${color} 62%, transparent)`,
                }}
              />
              {item.secondary !== undefined && (
                <span
                  className="w-full rounded-b-[2px] opacity-70"
                  style={{
                    height: Math.max(1, (item.secondary / max) * (height - 26)),
                    backgroundColor: secondaryColor,
                  }}
                />
              )}
            </button>
          );
        })}
      </div>
      <div className="mt-1.5 flex gap-1.5">
        {data.map((item) => (
          <span
            key={item.id}
            className="flex-1 truncate text-center font-mono text-2xs text-ink-muted"
          >
            {item.label}
          </span>
        ))}
      </div>
    </div>
  );
}
