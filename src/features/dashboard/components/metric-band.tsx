import { Clock, Flame, Layers, Target } from "lucide-react";
import type { DashboardSnapshot } from "@/types";
import { formatMinutes } from "@/lib/format";
import { Heatmap } from "@/components/charts/heatmap";
import { RadialGauge } from "@/components/charts/radial-gauge";

export function MetricBand({ snapshot }: { snapshot: DashboardSnapshot }) {
  const { today, streak, totals } = snapshot;
  const goalRate = Math.round((today.minutes / Math.max(1, today.goalMinutes)) * 100);

  return (
    <section
      className="mb-7 grid grid-cols-1 divide-y divide-line rounded-md border border-line bg-surface sm:grid-cols-2 sm:divide-y-0 xl:grid-cols-4 xl:divide-x"
      aria-label="今日概况"
    >
      <div className="flex items-center gap-3.5 p-4">
        <RadialGauge value={goalRate} size={62} thickness={5} label="今日" />
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 text-2xs text-ink-muted">
            <Clock className="size-3" />
            今日学习
          </div>
          <div className="mt-1 flex items-baseline gap-1">
            <span className="type-numeral text-lg leading-none text-ink">{today.minutes}</span>
            <span className="text-2xs text-ink-muted">/ {today.goalMinutes} 分钟</span>
          </div>
          <p className="mt-1 text-2xs text-ink-muted">
            已完成 {today.items} 题 · 正确率 {today.accuracy}%
          </p>
        </div>
      </div>

      <div className="p-4">
        <div className="flex items-center gap-1.5 text-2xs text-ink-muted">
          <Flame className="size-3 text-ember" />
          连续学习
        </div>
        <div className="mt-1 flex items-baseline gap-1.5">
          <span className="type-numeral text-lg leading-none text-ink">{streak.current}</span>
          <span className="text-2xs text-ink-muted">天</span>
          <span className="ml-1 text-2xs text-ink-muted">最长 {streak.longest} 天</span>
        </div>
        <Heatmap cells={streak.recent} className="mt-2.5" max={60} />
      </div>

      <div className="p-4">
        <div className="flex items-center gap-1.5 text-2xs text-ink-muted">
          <Layers className="size-3" />
          词汇掌握
        </div>
        <div className="mt-1 flex items-baseline gap-1">
          <span className="type-numeral text-lg leading-none text-ink">
            {snapshot.mastery.vocabulary.mastered}
          </span>
          <span className="text-2xs text-ink-muted">/ {totals.words} 词</span>
        </div>
        <p className="mt-1 text-2xs text-ink-muted">
          复习中 {snapshot.mastery.vocabulary.reviewing} · 学习中{" "}
          {snapshot.mastery.vocabulary.learning}
        </p>
        <div className="mt-2 flex h-1 overflow-hidden rounded-full bg-sunken">
          {(["mastered", "reviewing", "learning", "new", "lapsed"] as const).map((key) => {
            const value = snapshot.mastery.vocabulary[key];
            const colors: Record<string, string> = {
              mastered: "var(--success)",
              reviewing: "var(--brand)",
              learning: "var(--ember)",
              new: "var(--line-strong)",
              lapsed: "var(--danger)",
            };
            return (
              <span
                key={key}
                style={{
                  width: `${(value / Math.max(1, totals.words)) * 100}%`,
                  backgroundColor: colors[key],
                }}
              />
            );
          })}
        </div>
      </div>

      <div className="p-4">
        <div className="flex items-center gap-1.5 text-2xs text-ink-muted">
          <Target className="size-3" />
          语法掌握
        </div>
        <div className="mt-1 flex items-baseline gap-1">
          <span className="type-numeral text-lg leading-none text-ink">
            {snapshot.mastery.grammar.mastered}
          </span>
          <span className="text-2xs text-ink-muted">/ {totals.grammarPoints} 个语法点</span>
        </div>
        <p className="mt-1 text-2xs text-ink-muted">
          复习中 {snapshot.mastery.grammar.reviewing} · 学习中{" "}
          {snapshot.mastery.grammar.learning}
        </p>
        <p className="mt-1.5 text-2xs text-ink-muted">
          累计学习 {formatMinutes(totals.minutes)} · {totals.days} 个学习日
        </p>
      </div>
    </section>
  );
}
