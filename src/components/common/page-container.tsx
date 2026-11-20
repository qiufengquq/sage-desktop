import { cn } from "@/lib/utils";

export function PageContainer({
  className,
  children,
}: {
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <div className={cn("mx-auto w-full max-w-[1240px] px-6 py-6 lg:px-9 lg:py-7", className)}>
      {children}
    </div>
  );
}

/** 页面内的分栏区域：主列 + 侧列，桌面端优先 */
export function PageSplit({
  children,
  aside,
  asideWidth = "22rem",
}: {
  children: React.ReactNode;
  aside: React.ReactNode;
  asideWidth?: string;
}) {
  return (
    <div
      className="grid gap-7 xl:gap-9"
      style={{ gridTemplateColumns: `minmax(0, 1fr) ${asideWidth}` }}
    >
      <div className="min-w-0">{children}</div>
      <div className="min-w-0">{aside}</div>
    </div>
  );
}
