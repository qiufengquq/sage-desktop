import { useEffect } from "react";
import { Toaster } from "sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useThemeSync } from "@/hooks/use-theme";
import { useUiStore } from "@/app/store/ui-store";

function ThemeBridge({ children }: { children: React.ReactNode }) {
  const resolved = useThemeSync();
  const accent = useUiStore((s) => s.accent);
  const density = useUiStore((s) => s.density);

  useEffect(() => {
    document.documentElement.dataset.accent = accent;
    document.documentElement.dataset.density = density;
  }, [accent, density]);

  return (
    <div data-theme={resolved} className="contents">
      {children}
    </div>
  );
}

export function AppProviders({ children }: { children: React.ReactNode }) {
  return (
    <TooltipProvider>
      <ThemeBridge>
        {children}
        <Toaster
          position="bottom-right"
          toastOptions={{
            className:
              "!bg-surface !text-ink !border !border-line !shadow-pop !rounded-md !text-[13px]",
          }}
        />
      </ThemeBridge>
    </TooltipProvider>
  );
}
