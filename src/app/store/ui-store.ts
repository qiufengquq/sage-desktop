import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { ThemeMode } from "@/types";

interface UiState {
  theme: ThemeMode;
  sidebarCollapsed: boolean;
  commandOpen: boolean;
  accent: "brand" | "ember";
  density: "comfortable" | "compact";
  setTheme: (theme: ThemeMode) => void;
  toggleTheme: () => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  toggleSidebar: () => void;
  setCommandOpen: (open: boolean) => void;
  setAccent: (accent: "brand" | "ember") => void;
  setDensity: (density: "comfortable" | "compact") => void;
}

export const useUiStore = create<UiState>()(
  persist(
    (set, get) => ({
      theme: "system",
      sidebarCollapsed: false,
      commandOpen: false,
      accent: "brand",
      density: "comfortable",
      setTheme: (theme) => set({ theme }),
      toggleTheme: () => {
        const current = get().theme;
        const resolved =
          current === "system"
            ? window.matchMedia("(prefers-color-scheme: dark)").matches
              ? "dark"
              : "light"
            : current;
        set({ theme: resolved === "dark" ? "light" : "dark" });
      },
      setSidebarCollapsed: (sidebarCollapsed) => set({ sidebarCollapsed }),
      toggleSidebar: () => set({ sidebarCollapsed: !get().sidebarCollapsed }),
      setCommandOpen: (commandOpen) => set({ commandOpen }),
      setAccent: (accent) => set({ accent }),
      setDensity: (density) => set({ density }),
    }),
    {
      name: "sage.ui",
      partialize: (state) => ({
        theme: state.theme,
        sidebarCollapsed: state.sidebarCollapsed,
        accent: state.accent,
        density: state.density,
      }),
    },
  ),
);

/** 解析出实际生效的主题（system 需要读系统偏好） */
export function resolveTheme(mode: ThemeMode): "light" | "dark" {
  if (mode !== "system") return mode;
  if (typeof window === "undefined") return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}
