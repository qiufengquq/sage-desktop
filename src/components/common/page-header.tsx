import { cn } from "@/lib/utils";

interface PageHeaderProps {
  title: string;
  description?: string;
  actions?: React.ReactNode;
  meta?: React.ReactNode;
  className?: string;
}

export function PageHeader({ title, description, actions, meta, className }: PageHeaderProps) {
  return (
    <header className={cn("mb-7 flex flex-wrap items-start justify-between gap-4", className)}>
      <div className="min-w-0 max-w-[62ch]">
        <h1 className="type-page-title text-ink">{title}</h1>
        {description && (
          <p className="mt-2 text-[13.5px] leading-relaxed text-ink-muted">{description}</p>
        )}
        {meta && <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1.5">{meta}</div>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  );
}

/** 页内小节标题：不做全大写 eyebrow，标题本身承担层级 */
export function SectionHeader({
  title,
  description,
  action,
  className,
}: {
  title: string;
  description?: string;
  action?: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("mb-3.5 flex items-end justify-between gap-4", className)}>
      <div>
        <h2 className="type-section-title text-ink">{title}</h2>
        {description && (
          <p className="mt-0.5 text-[12.5px] text-ink-muted">{description}</p>
        )}
      </div>
      {action}
    </div>
  );
}
