import type { LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { Progress } from "@/components/ui/progress";

export interface ProgressSegment {
  label: string;
  value: number;
  color: string;
}

interface ProgressCardProps {
  title: string;
  value: number;
  total: number;
  description?: string;
  icon?: LucideIcon;
  segments?: ProgressSegment[];
  meta?: string;
  className?: string;
  bare?: boolean;
}

export function ProgressCard({
  title,
  value,
  total,
  description,
  icon: Icon,
  segments,
  meta,
  className,
  bare = false,
}: ProgressCardProps) {
  const percent = total === 0 ? 0 : Math.round((value / total) * 100);

  return (
    <div
      className={cn(
        "flex flex-col gap-3",
        bare ? "" : "rounded-md border border-line bg-surface p-4",
        className,
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5">
            {Icon && <Icon className="size-3.5 text-ink-muted" />}
            <h3 className="text-[13.5px] font-medium text-ink">{title}</h3>
          </div>
          {description && <p className="mt-0.5 text-2xs text-ink-muted">{description}</p>}
        </div>
        <div className="shrink-0 text-right">
          <div className="type-numeral text-base leading-none text-ink">{percent}%</div>
          {meta && <div className="mt-1 text-2xs text-ink-muted">{meta}</div>}
        </div>
      </div>

      {segments && segments.length > 0 ? (
        <div className="flex h-1.5 w-full overflow-hidden rounded-full bg-sunken">
          {segments.map((segment) => (
            <span
              key={segment.label}
              title={`${segment.label} ${segment.value}`}
              style={{
                width: `${total === 0 ? 0 : (segment.value / total) * 100}%`,
                backgroundColor: segment.color,
              }}
              className="h-full first:rounded-l-full last:rounded-r-full"
            />
          ))}
        </div>
      ) : (
        <Progress value={percent} />
      )}

      {segments && segments.length > 0 && (
        <div className="flex flex-wrap items-center gap-x-3.5 gap-y-1">
          {segments.map((segment) => (
            <span
              key={segment.label}
              className="flex items-center gap-1.5 text-2xs text-ink-muted"
            >
              <span
                className="size-1.5 rounded-full"
                style={{ backgroundColor: segment.color }}
                aria-hidden
              />
              {segment.label}
              <span className="font-mono text-ink-soft">{segment.value}</span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
