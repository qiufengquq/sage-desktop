import { ArrowRight, Minus, TrendingDown, TrendingUp } from "lucide-react";
import type { AbilityProfile } from "@/types";
import { formatRelativeTime } from "@/lib/date";
import { RadarChart } from "@/components/charts/radar-chart";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

export function AbilityProfilePanel({
  profile,
  onGoAssessment,
}: {
  profile: AbilityProfile;
  onGoAssessment: () => void;
}) {
  return (
    <section className="rounded-md border border-line bg-surface p-5">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h2 className="type-section-title text-ink">能力画像</h2>
          <p className="mt-0.5 text-2xs text-ink-muted">
            {profile.sourceAssessmentId
              ? "有测评成绩的维度来自测评，其余由训练表现估算"
              : "尚无测评：以下分数为训练表现估算值"}{" "}
            · 更新于 {formatRelativeTime(profile.updatedAt)}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Badge variant="brand">当前 {profile.level}</Badge>
          <ArrowRight className="size-3 text-ink-muted" />
          <Badge variant="ember">目标 {profile.targetLevel}</Badge>
        </div>
      </div>

      <div className="mt-4 grid items-center gap-6 lg:grid-cols-[auto_minmax(0,1fr)]">
        <div className="relative mx-auto">
          <RadarChart
            axes={profile.axes.map((axis) => ({
              label: axis.label,
              value: axis.score,
              benchmark: axis.benchmark,
            }))}
            size={276}
          />
          <div className="absolute inset-0 flex flex-col items-center justify-center">
            <span className="type-numeral text-2xl leading-none text-ink">{profile.overall}</span>
            <span className="mt-1 text-[10px] text-ink-muted">综合分</span>
          </div>
        </div>

        <div className="min-w-0 space-y-3">
          <div className="grid grid-cols-2 gap-x-4 gap-y-1.5">
            {profile.axes.map((axis) => {
              const delta = axis.score - axis.benchmark;
              return (
                <div
                  key={axis.key}
                  className="flex items-center justify-between gap-2"
                  title={axis.source}
                >
                  <span className="flex items-center gap-1 truncate text-2xs text-ink-soft">
                    {axis.label}
                    {axis.confidence === "estimated" && (
                      <span className="text-[9px] text-ink-muted">估</span>
                    )}
                  </span>
                  <span className="flex shrink-0 items-center gap-1 font-mono text-2xs">
                    <span className="text-ink">{axis.score}</span>
                    <span
                      className={
                        delta > 0 ? "text-success" : delta < 0 ? "text-danger" : "text-ink-muted"
                      }
                    >
                      {delta > 0 ? (
                        <TrendingUp className="size-3" />
                      ) : delta < 0 ? (
                        <TrendingDown className="size-3" />
                      ) : (
                        <Minus className="size-3" />
                      )}
                    </span>
                  </span>
                </div>
              );
            })}
          </div>

          <div className="grid gap-2 sm:grid-cols-2">
            <div className="rounded-sm bg-success/[0.06] px-2.5 py-2">
              <span className="text-[10px] font-medium text-success">优势</span>
              <ul className="mt-1 space-y-0.5">
                {profile.strengths.map((line) => (
                  <li key={line} className="text-2xs leading-relaxed text-ink-soft">
                    {line}
                  </li>
                ))}
              </ul>
            </div>
            <div className="rounded-sm bg-warning/[0.07] px-2.5 py-2">
              <span className="text-[10px] font-medium text-warning">短板</span>
              <ul className="mt-1 space-y-0.5">
                {profile.weaknesses.map((line) => (
                  <li key={line} className="text-2xs leading-relaxed text-ink-soft">
                    {line}
                  </li>
                ))}
              </ul>
            </div>
          </div>

          <Button variant="outline" size="sm" onClick={onGoAssessment}>
            重新测评以更新画像
            <ArrowRight />
          </Button>
        </div>
      </div>
    </section>
  );
}
