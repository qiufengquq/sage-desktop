import { cn } from "@/lib/utils";

export interface RadarAxis {
  label: string;
  value: number;
  benchmark?: number;
}

interface RadarChartProps {
  axes: RadarAxis[];
  size?: number;
  className?: string;
  max?: number;
}

/** 能力雷达图：用户评分与同水平基准叠加，缺口一眼可见 */
export function RadarChart({ axes, size = 260, className, max = 100 }: RadarChartProps) {
  const padding = 34;
  const radius = (size - padding * 2) / 2;
  const center = size / 2;
  const count = axes.length;

  const pointAt = (index: number, ratio: number) => {
    const angle = (Math.PI * 2 * index) / count - Math.PI / 2;
    return {
      x: center + Math.cos(angle) * radius * ratio,
      y: center + Math.sin(angle) * radius * ratio,
    };
  };

  const toPolygon = (pick: (axis: RadarAxis) => number) =>
    axes
      .map((axis, i) => {
        const p = pointAt(i, Math.min(1, pick(axis) / max));
        return `${p.x.toFixed(1)},${p.y.toFixed(1)}`;
      })
      .join(" ");

  const levels = [0.25, 0.5, 0.75, 1];

  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      className={cn("overflow-visible", className)}
      role="img"
      aria-label="能力雷达图"
    >
      {levels.map((level) => (
        <polygon
          key={level}
          points={axes
            .map((_, i) => {
              const p = pointAt(i, level);
              return `${p.x.toFixed(1)},${p.y.toFixed(1)}`;
            })
            .join(" ")}
          fill="none"
          stroke="var(--line)"
          strokeWidth={1}
        />
      ))}

      {axes.map((axis, i) => {
        const p = pointAt(i, 1);
        const label = pointAt(i, 1.16);
        const anchor =
          Math.abs(label.x - center) < 6 ? "middle" : label.x > center ? "start" : "end";
        return (
          <g key={axis.label}>
            <line
              x1={center}
              y1={center}
              x2={p.x}
              y2={p.y}
              stroke="var(--line)"
              strokeWidth={1}
            />
            <text
              x={label.x}
              y={label.y}
              textAnchor={anchor}
              dominantBaseline="middle"
              className="fill-ink-muted"
              style={{ fontSize: 9.5 }}
            >
              {axis.label}
            </text>
          </g>
        );
      })}

      {axes.some((a) => a.benchmark !== undefined) && (
        <polygon
          points={toPolygon((axis) => axis.benchmark ?? 0)}
          fill="none"
          stroke="var(--ink-muted)"
          strokeWidth={1}
          strokeDasharray="3 3"
          opacity={0.75}
        />
      )}

      <polygon
        points={toPolygon((axis) => axis.value)}
        fill="color-mix(in oklab, var(--brand) 16%, transparent)"
        stroke="var(--brand)"
        strokeWidth={1.75}
        strokeLinejoin="round"
      />

      {axes.map((axis, i) => {
        const p = pointAt(i, Math.min(1, axis.value / max));
        return <circle key={axis.label} cx={p.x} cy={p.y} r={2.4} fill="var(--brand)" />;
      })}
    </svg>
  );
}
