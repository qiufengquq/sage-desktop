import { ArrowRight, Check, Gauge, Loader2 } from "lucide-react";
import type { DifficultyAdjustment, PlanAdvice, PlanItem, WeeklyGoal } from "@/types";
import { ACTIVITY_LABEL } from "@/types/learning";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Slider } from "@/components/ui/slider";
import { Progress } from "@/components/ui/progress";
import { SectionHeader } from "@/components/common/page-header";

/* --------------------------------- 今日计划 -------------------------------- */
export function DailyPlan({
  items,
  pendingItemId,
  onToggle,
  onStart,
}: {
  items: PlanItem[];
  pendingItemId: string | null;
  onToggle: (item: PlanItem) => void;
  onStart: (item: PlanItem) => void;
}) {
  const done = items.filter((i) => i.done).length;
  const totalMinutes = items.reduce((acc, i) => acc + i.minutes, 0);

  return (
    <section>
      <SectionHeader
        title="今日学习计划"
        description={`${done}/${items.length} 项完成 · 计划 ${totalMinutes} 分钟`}
      />
      <div className="rounded-md border border-line bg-surface px-4">
        {items.map((item) => {
          const busy = pendingItemId === item.id;
          return (
            <div
              key={item.id}
              className="flex items-start gap-3 border-b border-line/70 py-3 last:border-0"
            >
              <Checkbox
                checked={item.done}
                onCheckedChange={() => onToggle(item)}
                disabled={busy}
                aria-label={`标记「${item.title}」${item.done ? "未完成" : "完成"}`}
                className="mt-0.5"
              />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span
                    className={cn(
                      "text-[13.5px] font-medium",
                      item.done ? "text-ink-muted line-through decoration-line-strong" : "text-ink",
                    )}
                  >
                    {item.title}
                  </span>
                  <Badge variant="default">{ACTIVITY_LABEL[item.activity]}</Badge>
                  {item.status === "in-progress" && (
                    <Badge variant="brand">
                      <Loader2 className="size-3 animate-spin" />
                      进行中
                    </Badge>
                  )}
                </div>
                <p className="mt-1 max-w-[56ch] text-2xs leading-relaxed text-ink-muted">
                  {item.reason}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                <span className="font-mono text-2xs text-ink-muted">{item.minutes} 分</span>
                <Button size="icon-sm" variant="ghost" onClick={() => onStart(item)} aria-label="开始该项训练">
                  <ArrowRight />
                </Button>
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}

/* --------------------------------- 每周目标 -------------------------------- */
export function WeeklyGoals({ goals }: { goals: WeeklyGoal[] }) {
  return (
    <section>
      <SectionHeader title="本周目标" description="目标由画像与近期表现推算，可按周调整" />
      <div className="divide-y divide-line rounded-md border border-line bg-surface">
        {goals.map((goal) => {
          const percent = Math.min(100, Math.round((goal.current / Math.max(1, goal.target)) * 100));
          return (
            <div key={goal.id} className="px-4 py-2.5">
              <div className="flex items-baseline justify-between gap-2">
                <span className="text-[13px] text-ink">{goal.label}</span>
                <span className="font-mono text-2xs text-ink-muted">
                  <span className="text-ink">{goal.current}</span> / {goal.target} {goal.unit}
                </span>
              </div>
              <div className="mt-1.5">
                <Progress value={percent} className="h-1" />
              </div>
              <p className="mt-1 text-2xs text-ink-muted">{goal.rationale}</p>
            </div>
          );
        })}
      </div>
    </section>
  );
}

/* --------------------------------- AI 建议 -------------------------------- */
const ADVICE_TONE = {
  vocabulary: { label: "词汇", variant: "brand" as const },
  grammar: { label: "语法", variant: "success" as const },
  speaking: { label: "口语", variant: "ember" as const },
};

export function AdviceList({
  advice,
  onApply,
}: {
  advice: PlanAdvice[];
  onApply: (item: PlanAdvice) => void;
}) {
  return (
    <section>
      <SectionHeader
        title="学习建议"
        description="每条建议都附带判断依据，避免不可解释的推荐"
      />
      <div className="divide-y divide-line rounded-md border border-line bg-surface">
        {advice.map((item) => {
          const tone = ADVICE_TONE[item.kind];
          return (
            <article key={item.id} className="px-4 py-3.5">
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant={tone.variant}>{tone.label}</Badge>
                <h3 className="font-display text-[16px] leading-snug text-ink">{item.title}</h3>
              </div>
              <p className="mt-1.5 max-w-[68ch] text-[12.5px] leading-relaxed text-ink-soft">
                {item.body}
              </p>
              <ul className="mt-2 flex flex-wrap gap-x-4 gap-y-1">
                {item.evidence.map((line) => (
                  <li key={line} className="flex items-center gap-1.5 text-2xs text-ink-muted">
                    <span className="size-1 rounded-full bg-ink-muted/50" aria-hidden />
                    {line}
                  </li>
                ))}
              </ul>
              <Button
                variant="ghost"
                size="sm"
                className="mt-2 px-0 text-brand"
                onClick={() => onApply(item)}
              >
                {item.action.label}
                <ArrowRight />
              </Button>
            </article>
          );
        })}
      </div>
    </section>
  );
}

/* -------------------------------- 难度调整 -------------------------------- */
export function DifficultyControl({
  difficulty,
  value,
  onChange,
  onApply,
  busy,
}: {
  difficulty: DifficultyAdjustment;
  value: number;
  onChange: (value: number) => void;
  onApply: () => void;
  busy: boolean;
}) {
  const changed = Math.abs(value - difficulty.current) > 0.05;

  return (
    <section className="rounded-md border border-line bg-surface p-4">
      <div className="flex items-center gap-1.5">
        <Gauge className="size-3.5 text-ink-muted" />
        <h3 className="text-[13.5px] font-medium text-ink">学习难度</h3>
      </div>

      <div className="mt-2 flex items-baseline gap-1.5">
        <span className="type-numeral text-xl leading-none text-ink">{value.toFixed(1)}</span>
        <span className="text-2xs text-ink-muted">/ 5.0</span>
        {difficulty.requiresConfirmation && (
          <Badge variant="warning" className="ml-auto">
            建议 {difficulty.suggested.toFixed(1)}
          </Badge>
        )}
      </div>

      <Slider
        value={[value]}
        min={1}
        max={5}
        step={0.1}
        onValueChange={(next) => onChange(next[0] ?? value)}
        className="mt-3"
        aria-label="学习难度"
      />
      <div className="mt-1 flex justify-between font-mono text-[10px] text-ink-muted">
        <span>1.0 入门</span>
        <span>3.0 进阶</span>
        <span>5.0 精通</span>
      </div>

      <p className="mt-2 text-2xs leading-relaxed text-ink-muted">{difficulty.reason}</p>

      <div className="mt-3 flex items-center gap-2">
        <Button size="sm" disabled={!changed || busy} onClick={onApply}>
          {busy && <Loader2 className="animate-spin" />}
          应用难度
        </Button>
        {!changed && (
          <span className="flex items-center gap-1 text-2xs text-ink-muted">
            <Check className="size-3" />
            已是当前难度
          </span>
        )}
      </div>
    </section>
  );
}
