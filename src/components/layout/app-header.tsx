import { Link, useLocation } from "react-router";
import { ChevronRight, HardDrive, Moon, PanelLeftOpen, Search, Sun } from "lucide-react";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/app/store/ui-store";
import { useLearningSummary } from "@/hooks/use-learning-summary";
import { modKeyLabel } from "@/hooks/use-hotkey";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { findNavItem } from "./nav-config";

export function AppHeader() {
  const { pathname } = useLocation();
  const current = findNavItem(pathname);
  const theme = useUiStore((s) => s.theme);
  const toggleTheme = useUiStore((s) => s.toggleTheme);
  const collapsed = useUiStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const setCommandOpen = useUiStore((s) => s.setCommandOpen);
  const { profile } = useLearningSummary();
  const avatarLetter = profile.displayName.trim().slice(0, 1) || "S";

  const isDark =
    theme === "dark" ||
    (theme === "system" &&
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);

  return (
    <header className="flex h-(--header-h) shrink-0 items-center gap-3 border-b border-line bg-paper px-4">
      {collapsed && (
        <Button variant="ghost" size="icon-sm" onClick={toggleSidebar} aria-label="展开侧边导航">
          <PanelLeftOpen />
        </Button>
      )}

      <nav aria-label="面包屑" className="flex min-w-0 items-center gap-1.5 text-[13px]">
        <Link
          to="/"
          className="shrink-0 text-ink-muted transition-colors hover:text-ink"
        >
          Sage
        </Link>
        <ChevronRight className="size-3 shrink-0 text-ink-muted/60" />
        <span className="truncate font-medium text-ink">{current.label}</span>
        <span className="hidden truncate text-2xs text-ink-muted lg:inline">
          {current.labelEn}
        </span>
      </nav>

      <div className="ml-auto flex items-center gap-1.5">
        <button
          type="button"
          onClick={() => setCommandOpen(true)}
          className={cn(
            "group flex h-8 items-center gap-2 rounded-md bg-surface px-2.5 text-[13px] text-ink-muted transition-colors",
            "shadow-hairline hover:text-ink-soft",
          )}
          aria-label="打开全局搜索"
        >
          <Search className="size-3.5" />
          <span className="hidden md:inline">搜索或跳转</span>
          <kbd className="ml-1 hidden rounded-sm bg-sunken px-1.5 py-0.5 font-mono text-2xs md:inline">
            {modKeyLabel()} K
          </kbd>
        </button>

        <Button
          variant="ghost"
          size="icon-sm"
          onClick={toggleTheme}
          aria-label={isDark ? "切换到浅色主题" : "切换到深色主题"}
        >
          {isDark ? <Sun /> : <Moon />}
        </Button>

        <Badge variant="outline" className="hidden gap-1 px-2 py-1 xl:inline-flex">
          <HardDrive className="size-3" />
          仅本机
        </Badge>

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              className="ml-1 flex size-7 items-center justify-center rounded-full bg-brand-tint text-[11px] font-medium text-brand transition-colors hover:bg-brand-tint-strong"
              aria-label="账户菜单"
            >
              {avatarLetter}
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-52">
            <DropdownMenuLabel>
              {profile.displayName} · {profile.level} → {profile.targetLevel}
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem asChild>
              <Link to="/settings">账户与目标</Link>
            </DropdownMenuItem>
            <DropdownMenuItem asChild>
              <Link to="/settings">外观与提醒</Link>
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={toggleTheme}>
              {isDark ? "切换到浅色主题" : "切换到深色主题"}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </header>
  );
}
