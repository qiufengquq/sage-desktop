import { ArrowRight, RotateCcw, TrendingDown, TrendingUp } from "lucide-react";
import type { AssessmentOutcome } from "@/types";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { RadialGauge } from "@/components/charts/radial-gauge";
import { MasteryBar } from "@/components/common/mastery-bar";
import { cn } from "@/lib/utils";

const LEVEL_HINT: Record<string, string> = {
  A1: "入门级，优先建立高频词与基本句型",
  A2: "基础级，可以先补齐时态与介词搭配",
  B1: "进阶级，重点在从句与表达流畅度",
  B2: "中高级，可以进入学术表达与长句改写",
  C1: "高级，建议以真实语料与写作输出为主",
};

export function AssessmentOutcomeView({
  outcome,
  moduleTitle,
  onRetry,
  onExit,
  onTrain,
}: {
  outcome: AssessmentOutcome;
  moduleTitle: string;
  onRetry: () => void;
  onExit: () => void;
  onTrain: () => void;
}) {
  return (
    <div className="space-y-5">
      <div className="rounded-md border border-line bg-surface p-5">
        <div className="flex flex-wrap items-center gap-5">
          <RadialGauge value={outcome.score} size={90} thickness={7} label="综合" />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="font-display text-[22px] leading-tight text-ink">
                {moduleTitle} · {outcome.level}
              </h2>
              <Badge variant="brand">{outcome.score} 分</Badge>
            </div>
            <p className="mt-1.5 max-w-[56ch] text-[12.5px] leading-relaxed text-ink-soft">
              {outcome.recommendation}
            </p>
            <p className="mt-1 text-2xs text-ink-muted">
              {LEVEL_HINT[outcome.level] ?? "继续按当前节奏训练即可"}。
            </p>
          </div>
        </div>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <section className="rounded-md border border-line bg-surface p-4">
          <h3 className="text-[13.5px] font-medium text-ink">分项得分</h3>
          <div className="mt-3 space-y-2.5">
            {outcome.perSection
              .filter((s) => s.total > 0)
              .map((section) => (
                <div key={section.sectionId}>
                  <div className="mb-1 flex items-center justify-between">
                    <span className="text-2xs text-ink-soft">{section.title}</span>
                    <span className="font-mono text-2xs text-ink-muted">
                      {section.correct}/{section.total}
                    </span>
                  </div>
                  <MasteryBar value={section.score} showValue={false} />
                </div>
              ))}
          </div>
        </section>

        <section className="space-y-3">
          <div className="rounded-md border border-success/25 bg-success/[0.04] p-3.5">
            <div className="flex items-center gap-1.5 text-2xs font-medium text-success">
              <TrendingUp className="size-3" />
              优势
            </div>
            <ul className="mt-1.5 space-y-1">
              {outcome.strengths.map((line) => (
                <li key={line} className="text-2xs text-ink-soft">
                  {line}
                </li>
              ))}
            </ul>
          </div>
          <div
            className={cn(
              "rounded-md border p-3.5",
              outcome.weaknesses.length > 0
                ? "border-warning/30 bg-warning/[0.05]"
                : "border-line bg-surface",
            )}
          >
            <div className="flex items-center gap-1.5 text-2xs font-medium text-warning">
              <TrendingDown className="size-3" />
              待补强
            </div>
            {outcome.weaknesses.length === 0 ? (
              <p className="mt-1.5 text-2xs text-ink-muted">各项得分均衡，可以进入下一难度。</p>
            ) : (
              <ul className="mt-1.5 space-y-1">
                {outcome.weaknesses.map((line) => (
                  <li key={line} className="text-2xs text-ink-soft">
                    {line}
                  </li>
                ))}
              </ul>
            )}
          </div>
        </section>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" onClick={onRetry}>
          <RotateCcw />
          重新测试
        </Button>
        {outcome.followUp && (
          <Button onClick={onTrain}>
            {outcome.followUp.label}
            <ArrowRight />
          </Button>
        )}
        <Button variant="ghost" onClick={onExit} className="text-ink-muted">
          返回测评
        </Button>
      </div>
    </div>
  );
}
