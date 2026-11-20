import { ArrowRight } from "lucide-react";
import type { TrainingOption } from "@/types";
import { SectionHeader } from "@/components/common/page-header";

const TRACK_LABEL: Record<string, string> = {
  vocabulary: "词汇",
  grammar: "语法",
  speaking: "口语",
};

export function QuickStart({
  options,
  onStart,
}: {
  options: TrainingOption[];
  onStart: (option: TrainingOption) => void;
}) {
  return (
    <section>
      <SectionHeader title="快速开始" description="按当前薄弱项排序的 3 个训练入口" />
      <div className="space-y-2">
        {options.map((option) => (
          <button
            key={option.id}
            type="button"
            onClick={() => onStart(option)}
            className="group flex w-full items-center gap-3 rounded-md border border-line bg-surface px-3 py-2.5 text-left transition-colors hover:border-line-strong hover:bg-raised"
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5">
                <span className="text-[13px] font-medium text-ink">{option.title}</span>
                <span className="text-2xs text-ink-muted">{TRACK_LABEL[option.track]}</span>
              </div>
              <p className="mt-0.5 line-clamp-1 text-2xs text-ink-muted">{option.description}</p>
            </div>
            <div className="flex shrink-0 items-center gap-2">
              <span className="font-mono text-2xs text-ink-muted">{option.minutes} 分钟</span>
              <ArrowRight className="size-3.5 text-ink-muted transition-transform group-hover:translate-x-0.5" />
            </div>
          </button>
        ))}
      </div>
    </section>
  );
}
