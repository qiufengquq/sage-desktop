import { useState } from "react";
import { useNavigate } from "react-router";
import { ArrowRight, CalendarRange } from "lucide-react";
import type { AnalyticsData, DailyStat } from "@/types";
import { analyticsService } from "@/services/api";
import { useQuery } from "@/hooks/use-query";
import { formatMonthDay, formatDateShort } from "@/lib/date";
import { formatMinutes } from "@/lib/format";
import { cn } from "@/lib/utils";
import { PageContainer } from "@/components/common/page-container";
import { PageHeader, SectionHeader } from "@/components/common/page-header";
import { ErrorState, LoadingState } from "@/components/common/states";
import { ChartCard } from "@/components/common/chart-card";
import { StatCard } from "@/components/common/stat-card";
import { MasteryBar } from "@/components/common/mastery-bar";
import { LineChart } from "@/components/charts/line-chart";
import { BarChart } from "@/components/charts/bar-chart";
import { Heatmap } from "@/components/charts/heatmap";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

const RANGES = [7, 14, 30] as const;

/** 正确率变化：区间后半段 vs 前半段，没有数据时返回 0 而不是编造 */
function accuracyDeltaOf(daily: DailyStat[]): number {
  if (daily.length < 2) return 0;
  const mid = Math.floor(daily.length / 2);
  const summarize = (days: DailyStat[]) => {
    const items = days.reduce((acc, day) => acc + day.items, 0);
    const correct = days.reduce((acc, day) => acc + day.correct, 0);
    return items === 0 ? null : Math.round((correct / items) * 100);
  };
  const earlier = summarize(daily.slice(0, mid));
  const later = summarize(daily.slice(mid));
  if (earlier === null || later === null) return 0;
  return later - earlier;
}

export function AnalyticsPage() {
  const navigate = useNavigate();
  const [range, setRange] = useState<(typeof RANGES)[number]>(30);
  const { data, error, reload, initial, loading } = useQuery(
    (options) => analyticsService.getAnalytics(range, options),
    [range],
  );

  return (
    <PageContainer>
      <PageHeader
        title="学习数据"
        description="所有图表都由每日学习记录计算得出，没有为了好看而生成的曲线。切换时间范围会重新聚合。"
        meta={
          <span className="flex items-center gap-1 text-2xs text-ink-muted">
            <CalendarRange className="size-3" />
            统计区间：最近 {range} 天
          </span>
        }
        actions={
          <div className="flex rounded-md bg-sunken p-0.5" role="group" aria-label="时间范围">
            {RANGES.map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => setRange(option)}
                aria-pressed={range === option}
                className={cn(
                  "rounded-sm px-2.5 py-1 font-mono text-2xs transition-colors",
                  range === option
                    ? "bg-surface text-ink shadow-hairline"
                    : "text-ink-muted hover:text-ink-soft",
                )}
              >
                {option} 天
              </button>
            ))}
          </div>
        }
      />

      {initial && <LoadingState variant="chart" className="mb-4" />}
      {error && <ErrorState error={error} onRetry={reload} />}

      {data && (
        <div className={cn("space-y-6 transition-opacity", loading && "opacity-60")}>
          <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
            <StatCard
              label="累计学习时长"
              value={formatMinutes(data.totals.minutes).replace(" ", "")}
              icon={CalendarRange}
              hint={`活跃 ${data.totals.activeDays} 天`}
            />
            <StatCard
              label="完成题目"
              value={`${data.totals.items}`}
              unit="题"
              hint={`平均 ${data.totals.avgMinutes} 分钟/天`}
            />
            <StatCard
              label="平均正确率"
              value={`${data.totals.accuracy}`}
              unit="%"
              delta={accuracyDeltaOf(data.daily)}
              deltaLabel="%"
            />
            <StatCard
              label="最高单日"
              value={data.totals.bestDay ? `${data.totals.bestDay.minutes}` : "—"}
              unit="分钟"
              hint={data.totals.bestDay ? formatDateShort(data.totals.bestDay.date) : undefined}
            />
          </div>

          <DailySection data={data} />

          <div className="grid gap-5 lg:grid-cols-2">
            <MasteryTrendSection data={data} />
            <ForgettingSection data={data} />
          </div>

          <div className="grid gap-5 lg:grid-cols-2">
            <WeeklySection data={data} />
            <TimeDistributionSection data={data} />
          </div>

          <div className="grid gap-5 lg:grid-cols-2">
            <ReviewSection
              data={data}
              onOpen={(id) => navigate(`/knowledge?id=${id}`)}
              onReview={() => navigate("/review")}
            />
            <MasteredSection data={data} onOpen={(id) => navigate(`/knowledge?id=${id}`)} />
          </div>
        </div>
      )}
    </PageContainer>
  );
}

function DailySection({ data }: { data: AnalyticsData }) {
  return (
    <ChartCard
      title="每日学习记录"
      description="柱状为学习分钟数，下方为同期正确率"
      value={formatMinutes(data.totals.minutes)}
      footer={
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-2xs text-ink-muted">
            共 {data.daily.length} 天记录，其中 {data.totals.activeDays} 天有学习行为
          </span>
          <Heatmap
            cells={data.daily.map((d) => ({ date: d.date, minutes: d.minutes }))}
            className="w-40"
          />
        </div>
      }
    >
      <LineChart
        labels={data.daily.map((d) => formatMonthDay(d.date))}
        series={[
          {
            id: "minutes",
            label: "分钟",
            color: "var(--brand)",
            values: data.daily.map((d) => d.minutes),
          },
        ]}
        height={190}
        unit=" 分钟"
      />
    </ChartCard>
  );
}

function MasteryTrendSection({ data }: { data: AnalyticsData }) {
  const labels = data.trends[0]?.series.map((p) => formatMonthDay(p.date)) ?? [];
  return (
    <ChartCard
      title="掌握趋势"
      description="词汇 / 语法 / 口语三项的掌握度变化"
      value={`${data.trends[0]?.current ?? 0}`}
      unit="%"
    >
      <LineChart
        labels={labels}
        series={data.trends.map((trend, index) => ({
          id: trend.kind,
          label: trend.label,
          color:
            index === 0 ? "var(--brand)" : index === 1 ? "var(--ember)" : "var(--info)",
          values: trend.series.map((p) => p.value),
        }))}
        height={200}
        yMax={100}
        unit="%"
      />
      <div className="mt-2 grid grid-cols-3 gap-2">
        {data.trends.map((trend) => (
          <div key={trend.kind} className="rounded-sm bg-sunken/50 px-2 py-1.5">
            <div className="text-[10px] text-ink-muted">{trend.label}</div>
            <div className="mt-0.5 flex items-baseline gap-1">
              <span className="type-numeral text-[13px] text-ink">{trend.current}%</span>
              <span
                className={cn(
                  "font-mono text-[10px]",
                  trend.delta > 0
                    ? "text-success"
                    : trend.delta < 0
                      ? "text-danger"
                      : "text-ink-muted",
                )}
              >
                {trend.delta > 0 ? `+${trend.delta}` : trend.delta}
              </span>
            </div>
          </div>
        ))}
      </div>
    </ChartCard>
  );
}

function ForgettingSection({ data }: { data: AnalyticsData }) {
  return (
    <ChartCard
      title="遗忘曲线"
      description="理论留存 vs 你的实际留存"
      value={`${data.forgettingCurve[data.forgettingCurve.length - 1]?.actual ?? 0}`}
      unit="% 长期留存"
    >
      <LineChart
        labels={data.forgettingCurve.map((p) =>
          p.hours < 24 ? `${p.hours}h` : `${Math.round(p.hours / 24)}d`,
        )}
        series={[
          {
            id: "expected",
            label: "理论留存",
            color: "var(--ink-muted)",
            values: data.forgettingCurve.map((p) => p.expected),
            dashed: true,
          },
          {
            id: "actual",
            label: "你的留存",
            color: "var(--brand)",
            values: data.forgettingCurve.map((p) => p.actual),
          },
        ]}
        height={200}
        yMax={100}
        unit="%"
      />
      <p className="mt-2 text-2xs leading-relaxed text-ink-muted">
        {(() => {
          const day1 = data.forgettingCurve.find((point) => point.hours === 24);
          if (!day1) return "留存曲线由当前记忆强度推算，用于安排复习节奏。";
          const diff = day1.actual - day1.expected;
          return diff >= 0
            ? `24 小时留存 ${day1.actual}%，比理论值高 ${diff} 个百分点，说明间隔复习起了作用。`
            : `24 小时留存 ${day1.actual}%，比理论值低 ${Math.abs(diff)} 个百分点，建议缩短复习间隔。`;
        })()}
      </p>
    </ChartCard>
  );
}

function WeeklySection({ data }: { data: AnalyticsData }) {
  return (
    <ChartCard
      title="每周学习统计"
      description="按自然周聚合的时长与正确率"
      footer={
        <div className="grid grid-cols-5 gap-1.5">
          {data.weekly.map((week) => (
            <div key={week.weekStart} className="text-center">
              <div className="font-mono text-[10px] text-ink-muted">{week.label}</div>
              <div className="mt-0.5 font-mono text-2xs text-ink-soft">{week.accuracy}%</div>
              <div className="text-[10px] text-ink-muted">{week.activeDays} 天</div>
            </div>
          ))}
        </div>
      }
    >
      <BarChart
        data={data.weekly.map((week) => ({
          id: week.weekStart,
          label: week.label.replace("第 ", "").replace(" 周", "W"),
          value: week.minutes,
        }))}
        height={150}
        unit=" 分"
      />
    </ChartCard>
  );
}

function TimeDistributionSection({ data }: { data: AnalyticsData }) {
  const max = Math.max(...data.timeDistribution.map((b) => b.minutes), 1);
  return (
    <ChartCard
      title="学习时间分布"
      description="横条长度为时长，圆点为该时段正确率"
      value={`${data.timeDistribution.length}`}
      unit="个时段"
    >
      <div className="space-y-2">
        {data.timeDistribution.map((bucket) => (
          <div key={bucket.label} className="flex items-center gap-3">
            <span className="w-12 shrink-0 font-mono text-2xs text-ink-muted">
              {bucket.label}
            </span>
            <div className="relative h-4 flex-1">
              <span
                className="absolute inset-y-1 left-0 rounded-sm bg-brand/70"
                style={{ width: `${(bucket.minutes / max) * 100}%` }}
              />
              <span
                className="absolute top-1/2 size-1.5 -translate-y-1/2 rounded-full bg-ember"
                style={{ left: `${bucket.accuracy}%` }}
                title={`正确率 ${bucket.accuracy}%`}
              />
            </div>
            <span className="w-14 shrink-0 text-right font-mono text-2xs text-ink-soft">
              {bucket.minutes} 分
            </span>
            <Badge
              variant={bucket.accuracy >= 85 ? "success" : bucket.accuracy >= 75 ? "default" : "warning"}
              className="w-11 justify-center"
            >
              {bucket.accuracy}%
            </Badge>
          </div>
        ))}
      </div>
      <p className="mt-3 text-2xs leading-relaxed text-ink-muted">
        {(() => {
          const active = data.timeDistribution.filter((bucket) => bucket.minutes > 0);
          if (active.length === 0) return "还没有足够数据判断高效时段，学习 3 天后这里会给出建议。";
          const best = [...active].sort((a, b) => b.accuracy - a.accuracy)[0];
          const worst = [...active].sort((a, b) => a.accuracy - b.accuracy)[0];
          return `${best?.label} 时段正确率最高（${best?.accuracy}%），建议把新词学习排在这里；${worst?.label} 时段为 ${worst?.accuracy}%，适合做机械复习。`;
        })()}
      </p>
    </ChartCard>
  );
}

function ReviewSection({
  data,
  onOpen,
  onReview,
}: {
  data: AnalyticsData;
  onOpen: (id: string) => void;
  onReview: () => void;
}) {
  const due = data.reviewQueue.filter((r) => r.state !== "mastered").slice(0, 8);
  return (
    <section>
      <SectionHeader
        title="待复习知识"
        description={`${data.reviewQueue.length} 项在队列中`}
        action={
          <Button size="sm" variant="ghost" onClick={onReview} disabled={due.length === 0}>
            去复习
            <ArrowRight />
          </Button>
        }
      />
      <div className="divide-y divide-line rounded-md border border-line bg-surface">
        {due.map((item) => (
          <button
            key={item.id}
            type="button"
            onClick={() => onOpen(item.entityId)}
            className="flex w-full items-center gap-3 px-3.5 py-2 text-left transition-colors hover:bg-raised"
          >
            <div className="min-w-0 flex-1">
              <div className="truncate text-[12.5px] text-ink">{item.entityLabel}</div>
              <div className="mt-1 flex items-center gap-2">
                <MasteryBar value={item.strength * 100} showValue={false} className="w-16" />
                <span className="text-[10px] text-ink-muted">
                  {item.overdueDays > 0 ? `逾期 ${item.overdueDays} 天` : "今日到期"}
                </span>
              </div>
            </div>
            <Badge variant="outline">{item.kind === "word" ? "词汇" : item.kind === "grammar" ? "语法" : "句子"}</Badge>
          </button>
        ))}
      </div>
    </section>
  );
}

function MasteredSection({
  data,
  onOpen,
}: {
  data: AnalyticsData;
  onOpen: (id: string) => void;
}) {
  const mastered = data.mastered.slice(0, 8);
  return (
    <section>
      <SectionHeader
        title="已掌握知识"
        description={`${data.mastered.length} 项达到掌握标准`}
      />
      <div className="divide-y divide-line rounded-md border border-line bg-surface">
        {mastered.map((item) => (
          <button
            key={item.id}
            type="button"
            onClick={() => onOpen(item.entityId)}
            className="flex w-full items-center gap-3 px-3.5 py-2 text-left transition-colors hover:bg-raised"
          >
            <div className="min-w-0 flex-1">
              <div className="truncate text-[12.5px] text-ink">{item.entityLabel}</div>
              <div className="text-[10px] text-ink-muted">
                记忆强度 {(item.strength * 100).toFixed(0)}% · 上次复习{" "}
                {item.lastReviewedAt ? formatDateShort(item.lastReviewedAt) : "—"}
              </div>
            </div>
            <Badge variant="success">已掌握</Badge>
          </button>
        ))}
        {mastered.length === 0 && (
          <div className="px-3.5 py-6 text-center text-2xs text-ink-muted">
            还没有达到掌握标准的条目，先完成今天的复习队列。
          </div>
        )}
      </div>
    </section>
  );
}

