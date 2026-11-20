import { useId } from "react";
import { cn } from "@/lib/utils";

interface SparklineProps {
  values: number[];
  className?: string;
  stroke?: string;
  height?: number;
  fill?: boolean;
}

/** 迷你趋势线：只表达方向，不加坐标轴 */
export function Sparkline({
  values,
  className,
  stroke = "var(--brand)",
  height = 30,
  fill = false,
}: SparklineProps) {
  const gradientId = useId();
  if (values.length < 2) return <div style={{ height }} className={className} />;

  const width = 100;
  const max = Math.max(...values, 1);
  const min = Math.min(...values, 0);
  const span = max - min || 1;
  const step = width / (values.length - 1);

  const points = values.map((v, i) => {
    const x = i * step;
    const y = height - ((v - min) / span) * (height - 4) - 2;
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  });

  const path = `M${points.join(" L")}`;
  const area = `${path} L${width},${height} L0,${height} Z`;

  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      className={cn("w-full", className)}
      style={{ height }}
      role="img"
      aria-label="趋势线"
    >
      {fill && (
        <>
          <defs>
            <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={stroke} stopOpacity="0.18" />
              <stop offset="100%" stopColor={stroke} stopOpacity="0" />
            </linearGradient>
          </defs>
          <path d={area} fill={`url(#${gradientId})`} />
        </>
      )}
      <path
        d={path}
        fill="none"
        stroke={stroke}
        strokeWidth={1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}
