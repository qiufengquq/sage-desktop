import { Outlet } from "react-router";
import { useCallback } from "react";
import { useUiStore } from "@/app/store/ui-store";
import { useHotkey } from "@/hooks/use-hotkey";
import { AppSidebar } from "./app-sidebar";
import { AppHeader } from "./app-header";
import { CommandPalette } from "./command-palette";

export function AppShell() {
  const setCommandOpen = useUiStore((s) => s.setCommandOpen);

  const openCommand = useCallback(
    (event: KeyboardEvent) => {
      event.preventDefault();
      setCommandOpen(true);
    },
    [setCommandOpen],
  );

  useHotkey({ key: "k", meta: true }, openCommand);
  useHotkey({ key: "k", ctrl: true }, openCommand);

  return (
    <div className="flex h-screen overflow-hidden bg-paper text-ink">
      <a
        href="#main-content"
        onClick={(event) => {
          // HashRouter 下改 hash 会被当成路由跳转（变成 404），这里改为脚本聚焦
          event.preventDefault();
          const main = document.getElementById("main-content");
          if (!main) return;
          main.scrollIntoView();
          main.focus();
        }}
        className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 focus:rounded-sm focus:bg-surface focus:px-3 focus:py-2 focus:text-sm focus:shadow-pop"
      >
        跳到主内容
      </a>
      <AppSidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <AppHeader />
        <main
          id="main-content"
          tabIndex={-1}
          className="min-h-0 flex-1 overflow-y-auto focus:outline-none"
        >
          <Outlet />
        </main>
      </div>
      <CommandPalette />
    </div>
  );
}
