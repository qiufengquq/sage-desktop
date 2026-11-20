import { NavLink } from "react-router";
import { Flame, PanelLeftClose, PanelLeftOpen } from "lucide-react";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/app/store/ui-store";
import { useLearningSummary } from "@/hooks/use-learning-summary";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { NAV_ITEMS } from "./nav-config";

export function AppSidebar() {
  const collapsed = useUiStore((s) => s.sidebarCollapsed);
  const toggle = useUiStore((s) => s.toggleSidebar);
  const { streak, dueCount } = useLearningSummary();

  return (
    <aside
      className={cn(
        "relative z-20 flex shrink-0 flex-col border-r border-line bg-paper transition-[width] duration-200 ease-instrument",
        collapsed ? "w-(--sidebar-w-collapsed)" : "w-(--sidebar-w)",
      )}
      aria-label="主导航"
    >
      {/* 品牌区 */}
      <div className="flex h-(--header-h) items-center gap-2.5 px-4">
        <div className="flex size-7 shrink-0 items-center justify-center rounded-sm bg-brand text-brand-ink">
          <span className="font-display text-[15px] leading-none font-medium">S</span>
        </div>
        {!collapsed && (
          <div className="min-w-0">
            <div className="font-display text-[15px] leading-none tracking-tight text-ink">
              Sage
            </div>
            <div className="mt-0.5 text-2xs text-ink-muted">英语学习 · 桌面版</div>
          </div>
        )}
      </div>

      <nav className="mt-2 flex flex-1 flex-col gap-0.5 px-2">
        {NAV_ITEMS.map((item) => {
          const Icon = item.icon;
          const link = (
            <NavLink
              key={item.id}
              to={item.path}
              end={item.path === "/"}
              className={({ isActive }) =>
                cn(
                  "group relative flex items-center gap-2.5 rounded-md px-2.5 py-2 text-[13.5px] transition-colors duration-150",
                  isActive
                    ? "bg-surface text-ink shadow-hairline"
                    : "text-ink-muted hover:bg-sunken/70 hover:text-ink-soft",
                )
              }
            >
              {({ isActive }) => (
                <>
                  <span
                    aria-hidden
                    className={cn(
                      "absolute top-1/2 left-0 h-4 w-[2px] -translate-y-1/2 rounded-full transition-opacity",
                      isActive ? "bg-brand opacity-100" : "opacity-0",
                    )}
                  />
                  <Icon
                    className={cn(
                      "size-4 shrink-0 transition-colors",
                      isActive ? "text-brand" : "text-ink-muted group-hover:text-ink-soft",
                    )}
                    strokeWidth={isActive ? 2.1 : 1.7}
                  />
                  {!collapsed && <span className="truncate">{item.label}</span>}
                  {item.id === "review" && dueCount > 0 && (
                    <span
                      className={cn(
                        "ml-auto shrink-0 rounded-full bg-ember/12 font-mono text-[10px] leading-none text-ember",
                        collapsed ? "absolute top-1.5 right-1.5 size-1.5 p-0" : "px-1.5 py-0.5",
                      )}
                      aria-label={`${dueCount} 项到期`}
                    >
                      {collapsed ? "" : dueCount > 99 ? "99+" : dueCount}
                    </span>
                  )}
                </>
              )}
            </NavLink>
          );

          return collapsed ? (
            <Tooltip key={item.id}>
              <TooltipTrigger asChild>{link}</TooltipTrigger>
              <TooltipContent side="right">
                {item.label}
                <span className="ml-1 text-paper/55">{item.labelEn}</span>
              </TooltipContent>
            </Tooltip>
          ) : (
            link
          );
        })}
      </nav>

      {/* 连续学习 */}
      {!collapsed && (
        <div className="mx-3 mb-2 rounded-md border border-line px-3 py-2.5">
          <div className="flex items-center gap-1.5 text-2xs text-ink-muted">
            <Flame className="size-3 text-ember" />
            连续学习
          </div>
          <div className="mt-0.5 flex items-baseline gap-1">
            <span className="type-numeral text-xl text-ink">{streak.current}</span>
            <span className="text-2xs text-ink-muted">天 · 最长 {streak.longest} 天</span>
          </div>
        </div>
      )}

      <div className="border-t border-line p-2">
        <Button
          variant="ghost"
          size={collapsed ? "icon-sm" : "sm"}
          onClick={toggle}
          className={cn("w-full justify-start text-ink-muted", collapsed && "justify-center")}
          aria-label={collapsed ? "展开侧边栏" : "折叠侧边栏"}
        >
          {collapsed ? <PanelLeftOpen /> : <PanelLeftClose />}
          {!collapsed && <span className="text-[13px]">折叠导航</span>}
        </Button>
      </div>
    </aside>
  );
}
