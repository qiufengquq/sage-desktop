import { useEffect, useState } from "react";
import { resolveTheme, useUiStore } from "@/app/store/ui-store";

/** 把主题状态同步到 <html>，并返回当前实际生效的主题 */
export function useThemeSync() {
  const theme = useUiStore((s) => s.theme);
  const [resolved, setResolved] = useState<"light" | "dark">(() => resolveTheme(theme));

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");

    const apply = () => {
      const next = resolveTheme(theme);
      setResolved(next);
      const root = document.documentElement;
      root.classList.toggle("dark", next === "dark");
      root.style.colorScheme = next;
    };

    apply();
    if (theme === "system") {
      media.addEventListener("change", apply);
      return () => media.removeEventListener("change", apply);
    }
    return undefined;
  }, [theme]);

  return resolved;
}
