import { useMemo, useState } from "react";
import { cn } from "@/lib/utils";
import { useElementWidth } from "@/hooks/use-element-width";

export interface LineSeries {
  id: string;
  label: string;
  color: string;
  values: number[];
  dashed?: boolean;
}

interface LineChartProps {
  series: LineSeries[];
  labels: string[];
  height?: number;
  yMax?: number;
  yMin?: number;
  unit?: string;
  className?: string;
}

/**
 * 手绘 SVG 折线图。
 * viewBox 宽度取容器实测宽度，坐标与像素 1:1，文字不会被非等比缩放拉变形。
 */
export function LineChart({
  series,
  labels,
  height = 200,
  yMax,
  yMin = 0,
  unit = "",
  className,
}: LineChartProps) {
  const [hover, setHover] = useState<number | null>(null);
  const { ref, width } = useElementWidth<HTMLDivElement>();

  const max = useMemo(() => {
    const dataMax = Math.max(...series.flatMap((s) => s.values), 1);
    return yMax ?? Math.ceil((dataMax * 1.12) / 10) * 10;
  }, [series, yMax]);

  const count = labels.length;
  const padLeft = 34;
  const padRight = 10;
  const padTop = 12;
  const padBottom = 24;
  const plotW = Math.max(10, width - padLeft - padRight);
  const plotH = Math.max(10, height - padTop - padBottom);

  const xAt = (i: number) => padLeft + (count <= 1 ? plotW / 2 : (i / (count - 1)) * plotW);
  const yAt = (value: number) =>
    padTop + plotH - ((value - yMin) / (max - yMin || 1)) * plotH;

  const gridLines = 4;
  const labelStep = Math.max(1, Math.ceil(count / Math.max(4, Math.floor(width / 78))));

  return (
    <div className={cn("relative w-full", className)} ref={ref}>
      <svg
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        className="block max-w-full"
        onMouseLeave={() => setHover(null)}
        onMouseMove={(event) => {
          const rect = event.currentTarget.getBoundingClientRect();
          const x = ((event.clientX - rect.left) / rect.width) * width;
          const index = Math.round(((x - padLeft) / plotW) * (count - 1));
          setHover(Math.min(count - 1, Math.max(0, index)));
        }}
        role="img"
        aria-label="趋势折线图"
      >
        {Array.from({ length: gridLines + 1 }, (_, i) => {
          const value = yMin + ((max - yMin) / gridLines) * i;
          const y = yAt(value);
          return (
            <g key={i}>
              <line
                x1={padLeft}
                x2={width - padRight}
                y1={y}
                y2={y}
                stroke="var(--line)"
                strokeWidth={1}
                strokeDasharray={i === 0 ? undefined : "2 5"}
              />
              <text
                x={padLeft - 8}
                y={y + 3}
                textAnchor="end"
                fill="var(--ink-muted)"
                style={{ fontSize: 10, fontFamily: "var(--font-mono)" }}
              >
                {Math.round(value)}
              </text>
            </g>
          );
        })}

        {labels.map((label, i) =>
          i % labelStep === 0 ? (
            <text
              key={`${label}-${i}`}
              x={xAt(i)}
              y={height - 6}
              textAnchor="middle"
              fill="var(--ink-muted)"
              style={{ fontSize: 10 }}
            >
              {label}
            </text>
          ) : null,
        )}

        {series.map((s) => {
          const path = s.values
            .map((v, i) => `${i === 0 ? "M" : "L"}${xAt(i).toFixed(1)},${yAt(v).toFixed(1)}`)
            .join(" ");
          return (
            <path
              key={s.id}
              d={path}
              fill="none"
              stroke={s.color}
              strokeWidth={1.75}
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeDasharray={s.dashed ? "4 3" : undefined}
            />
          );
        })}

        {hover !== null && (
          <g>
            <line
              x1={xAt(hover)}
              x2={xAt(hover)}
              y1={padTop}
              y2={padTop + plotH}
              stroke="var(--line-strong)"
              strokeWidth={1}
            />
            {series.map((s) => (
              <circle
                key={s.id}
                cx={xAt(hover)}
                cy={yAt(s.values[hover] ?? 0)}
                r={2.8}
                fill="var(--surface)"
                stroke={s.color}
                strokeWidth={1.75}
              />
            ))}
          </g>
        )}
      </svg>

      {hover !== null && (
        <div
          className="pointer-events-none absolute top-0 z-10 min-w-28 rounded-sm bg-surface px-2 py-1.5 shadow-pop"
          style={{
            left: `${Math.min(88, Math.max(12, (xAt(hover) / width) * 100))}%`,
            transform: "translateX(-50%)",
          }}
        >
          <div className="text-2xs text-ink-muted">{labels[hover]}</div>
          {series.map((s) => (
            <div key={s.id} className="mt-0.5 flex items-center gap-1.5">
              <span
                className="size-1.5 rounded-full"
                style={{ backgroundColor: s.color }}
                aria-hidden
              />
              <span className="type-numeral text-[11px] text-ink">
                {s.values[hover]}
                {unit}
              </span>
              <span className="text-2xs text-ink-muted">{s.label}</span>
            </div>
          ))}
        </div>
      )}

      {series.length > 1 && (
        <div className="mt-1 flex flex-wrap items-center gap-4">
          {series.map((s) => (
            <span key={s.id} className="flex items-center gap-1.5 text-2xs text-ink-muted">
              <span
                className="h-0.5 w-4 rounded-full"
                style={{ backgroundColor: s.color }}
                aria-hidden
              />
              {s.label}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
