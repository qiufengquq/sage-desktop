import { cn } from "@/lib/utils";
import { formatMonthDay, formatWeekday } from "@/lib/date";

interface HeatmapCell {
  date: string;
  minutes: number;
}

/** 学习强度热力条：颜色深浅只映射真实分钟数 */
export function Heatmap({
  cells,
  className,
  max,
}: {
  cells: HeatmapCell[];
  className?: string;
  max?: number;
}) {
  const ceiling = max ?? Math.max(...cells.map((c) => c.minutes), 1);

  return (
    <div className={cn("flex items-end gap-1", className)} role="img" aria-label="最近学习强度">
      {cells.map((cell) => {
        const ratio = cell.minutes / ceiling;
        const opacity = cell.minutes === 0 ? 0.06 : 0.18 + ratio * 0.82;
        return (
          <span
            key={cell.date}
            className="group relative flex-1"
            title={`${formatMonthDay(cell.date)} ${formatWeekday(cell.date)} · ${cell.minutes} 分钟`}
          >
            <span
              className="block h-6 rounded-[2px] transition-transform group-hover:scale-y-110"
              style={{ backgroundColor: "var(--brand)", opacity }}
            />
            <span className="mt-1 block text-center font-mono text-[9px] text-ink-muted">
              {new Date(cell.date).getDate()}
            </span>
          </span>
        );
      })}
    </div>
  );
}
