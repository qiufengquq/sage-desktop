import { cn } from "@/lib/utils";

interface ChartCardProps {
  title: string;
  description?: string;
  value?: string;
  unit?: string;
  actions?: React.ReactNode;
  footer?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
}

export function ChartCard({
  title,
  description,
  value,
  unit,
  actions,
  footer,
  children,
  className,
  bodyClassName,
}: ChartCardProps) {
  return (
    <section className={cn("rounded-md border border-line bg-surface p-4", className)}>
      <header className="mb-3 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="text-[13.5px] font-medium text-ink">{title}</h3>
          {description && <p className="mt-0.5 text-2xs text-ink-muted">{description}</p>}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {value && (
            <span className="flex items-baseline gap-1">
              <span className="type-numeral text-base text-ink">{value}</span>
              {unit && <span className="text-2xs text-ink-muted">{unit}</span>}
            </span>
          )}
          {actions}
        </div>
      </header>

      <div className={cn("min-w-0", bodyClassName)}>{children}</div>

      {footer && <div className="mt-3 border-t border-line pt-2.5">{footer}</div>}
    </section>
  );
}
