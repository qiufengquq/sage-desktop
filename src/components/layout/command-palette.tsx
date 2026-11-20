import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { Command, CornerDownLeft, Moon, Search, Sun } from "lucide-react";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/app/store/ui-store";
import { modKeyLabel } from "@/hooks/use-hotkey";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { NAV_ITEMS } from "./nav-config";

interface CommandItem {
  id: string;
  label: string;
  hint: string;
  group: "页面" | "操作";
  icon: React.ComponentType<{ className?: string }>;
  run: () => void;
}

export function CommandPalette() {
  const open = useUiStore((s) => s.commandOpen);
  const setOpen = useUiStore((s) => s.setCommandOpen);
  const theme = useUiStore((s) => s.theme);
  const setTheme = useUiStore((s) => s.setTheme);
  const collapsed = useUiStore((s) => s.sidebarCollapsed);
  const setCollapsed = useUiStore((s) => s.setSidebarCollapsed);

  const navigate = useNavigate();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const items = useMemo<CommandItem[]>(() => {
    const pages: CommandItem[] = NAV_ITEMS.map((item) => ({
      id: `nav_${item.id}`,
      label: item.label,
      hint: item.description,
      group: "页面",
      icon: item.icon,
      run: () => navigate(item.path),
    }));

    const actions: CommandItem[] = [
      {
        id: "action_theme",
        label: theme === "dark" ? "切换到浅色主题" : "切换到深色主题",
        hint: "外观设置会保存到本地",
        group: "操作",
        icon: theme === "dark" ? Sun : Moon,
        run: () => setTheme(theme === "dark" ? "light" : "dark"),
      },
      {
        id: "action_sidebar",
        label: collapsed ? "展开侧边导航" : "折叠侧边导航",
        hint: "为主内容留出更大空间",
        group: "操作",
        icon: Command,
        run: () => setCollapsed(!collapsed),
      },
    ];

    return [...pages, ...actions];
  }, [navigate, theme, setTheme, collapsed, setCollapsed]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter(
      (item) =>
        item.label.toLowerCase().includes(q) ||
        item.hint.toLowerCase().includes(q) ||
        item.group.includes(q),
    );
  }, [items, query]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(0);
      const timer = setTimeout(() => inputRef.current?.focus(), 30);
      return () => clearTimeout(timer);
    }
    return undefined;
  }, [open]);

  useEffect(() => {
    setActive(0);
  }, [query]);

  const runItem = (item: CommandItem) => {
    setOpen(false);
    item.run();
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((i) => (i + 1) % Math.max(1, filtered.length));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((i) => (i - 1 + filtered.length) % Math.max(1, filtered.length));
    } else if (event.key === "Enter") {
      event.preventDefault();
      const target = filtered[active];
      if (target) runItem(target);
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent
        className="top-[18%] max-w-xl translate-y-0 gap-0 p-0"
        hideClose
        aria-describedby={undefined}
      >
        <DialogTitle className="sr-only">全局搜索</DialogTitle>
        <div className="flex items-center gap-2.5 border-b border-line px-4 py-3">
          <Search className="size-4 shrink-0 text-ink-muted" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder="搜索页面或操作…"
            className="flex-1 bg-transparent text-sm text-ink outline-none placeholder:text-ink-muted/80"
            aria-label="全局搜索"
          />
          <kbd className="rounded-sm bg-sunken px-1.5 py-0.5 font-mono text-2xs text-ink-muted">
            Esc
          </kbd>
        </div>

        <div className="max-h-80 overflow-y-auto p-1.5" role="listbox" aria-label="搜索结果">
          {filtered.length === 0 && (
            <div className="px-3 py-8 text-center text-[13px] text-ink-muted">
              没有匹配「{query}」的页面或操作
            </div>
          )}
          {filtered.map((item, index) => {
            const Icon = item.icon;
            const isActive = index === active;
            return (
              <button
                key={item.id}
                type="button"
                role="option"
                aria-selected={isActive}
                onMouseEnter={() => setActive(index)}
                onClick={() => runItem(item)}
                className={cn(
                  "flex w-full items-center gap-3 rounded-sm px-2.5 py-2 text-left transition-colors",
                  isActive ? "bg-sunken" : "hover:bg-sunken/60",
                )}
              >
                <Icon className={cn("size-4 shrink-0", isActive ? "text-brand" : "text-ink-muted")} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13.5px] text-ink">{item.label}</span>
                  <span className="block truncate text-2xs text-ink-muted">{item.hint}</span>
                </span>
                <span className="shrink-0 font-mono text-2xs text-ink-muted">{item.group}</span>
                {isActive && <CornerDownLeft className="size-3.5 shrink-0 text-ink-muted" />}
              </button>
            );
          })}
        </div>

        <div className="flex items-center justify-between border-t border-line px-4 py-2 text-2xs text-ink-muted">
          <span>↑↓ 选择，Enter 执行</span>
          <span className="font-mono">{modKeyLabel()} K 打开</span>
        </div>
      </DialogContent>
    </Dialog>
  );
}
