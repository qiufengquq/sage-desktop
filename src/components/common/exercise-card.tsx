import {
  ArrowDownUp,
  Headphones,
  Keyboard,
  Languages,
  ListChecks,
  MessagesSquare,
  Mic,
  Replace,
  SpellCheck,
  TextCursorInput,
  Link2,
  CheckSquare,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { TrainingOption } from "@/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

const ICONS: Record<string, LucideIcon> = {
  link: Link2,
  "list-checks": ListChecks,
  keyboard: Keyboard,
  headphones: Headphones,
  "text-cursor-input": TextCursorInput,
  languages: Languages,
  "check-square": CheckSquare,
  "arrow-down-up": ArrowDownUp,
  "spell-check": SpellCheck,
  replace: Replace,
  mic: Mic,
  "messages-square": MessagesSquare,
};

interface ExerciseCardProps {
  option: TrainingOption;
  onStart: (option: TrainingOption) => void;
  className?: string;
}

export function ExerciseCard({ option, onStart, className }: ExerciseCardProps) {
  const Icon = ICONS[option.icon] ?? ListChecks;

  return (
    <div
      className={cn(
        "group flex flex-col rounded-md border border-line bg-surface p-4 transition-colors duration-150 hover:border-line-strong",
        className,
      )}
    >
      <div className="flex items-start justify-between gap-2">
        <span className="flex size-8 items-center justify-center rounded-sm bg-brand-tint text-brand">
          <Icon className="size-4" />
        </span>
        <div className="flex items-center gap-1.5">
          <Badge variant="outline">难度 {option.difficulty}</Badge>
        </div>
      </div>

      <h3 className="mt-2.5 text-[13.5px] font-medium text-ink">{option.title}</h3>
      <p className="mt-1 line-clamp-2 flex-1 text-2xs leading-relaxed text-ink-muted">
        {option.description}
      </p>

      <div className="mt-3 flex items-center justify-between gap-2">
        <span className="font-mono text-2xs text-ink-muted">
          {option.minutes} 分钟 · {option.itemCount} 题
        </span>
        <Button size="sm" variant="outline" onClick={() => onStart(option)}>
          开始
        </Button>
      </div>

      {option.progress > 0 && (
        <div className="mt-2 flex items-center gap-2">
          <div className="h-0.5 flex-1 overflow-hidden rounded-full bg-sunken">
            <span
              className="block h-full rounded-full bg-brand/70"
              style={{ width: `${option.progress}%` }}
            />
          </div>
          <span className="font-mono text-[10px] text-ink-muted">{option.progress}%</span>
        </div>
      )}
    </div>
  );
}
