import { useNavigate } from "react-router";
import { ArrowRight, LineChart } from "lucide-react";
import type { PlanItemAction } from "@/types";
import { dashboardService } from "@/services/api";
import { planActionRoute, trainingRoute } from "@/lib/plan-target";
import { useQuery } from "@/hooks/use-query";
import { greeting, formatDateShort, formatWeekday } from "@/lib/date";
import { Button } from "@/components/ui/button";
import { PageContainer } from "@/components/common/page-container";
import { PageHeader } from "@/components/common/page-header";
import { ErrorState, LoadingState } from "@/components/common/states";
import { ProgressCard } from "@/components/common/progress-card";
import { MetricBand } from "./components/metric-band";
import { TodayTasks } from "./components/today-tasks";
import { AiNudge, ReviewPanel } from "./components/review-panel";
import { RecentRecords } from "./components/recent-records";
import { QuickStart } from "./components/quick-start";

export function DashboardPage() {
  const navigate = useNavigate();
  const { data, error, reload, initial } = useQuery(
    (options) => dashboardService.getDashboard(options),
    [],
  );

  const openAction = (action: PlanItemAction) => navigate(planActionRoute(action));

  const nextTask = data?.tasks.find((t) => t.status !== "done") ?? data?.tasks[0];

  return (
    <PageContainer>
      <PageHeader
        title={`${greeting()}，一舟`}
        description={`${formatDateShort(new Date())} ${formatWeekday(new Date())} · 当前 B1，目标 B2。今天还有 ${
          data ? data.tasks.filter((t) => t.status !== "done").length : "—"
        } 项任务待完成。`}
        actions={
          <>
            <Button
              variant="outline"
              onClick={() => navigate("/analytics")}
              aria-label="查看学习数据"
            >
              <LineChart />
              学习数据
            </Button>
            <Button
              disabled={!nextTask}
              onClick={() => (nextTask ? openAction(nextTask.action) : undefined)}
            >
              开始今日训练
              <ArrowRight />
            </Button>
          </>
        }
      />

      {initial && <LoadingState variant="cards" rows={4} className="mb-6" />}
      {error && <ErrorState error={error} onRetry={reload} className="mb-6" />}

      {data && (
        <>
          <MetricBand snapshot={data.snapshot} />

          <div className="grid grid-cols-1 gap-7 xl:grid-cols-[minmax(0,1fr)_20rem] xl:gap-8">
            <div className="min-w-0 space-y-7">
              <TodayTasks
                tasks={data.tasks}
                onStart={(task) => openAction(task.action)}
                onStartAll={() => {
                  if (nextTask) openAction(nextTask.action);
                }}
              />

              <section>
                <div className="mb-3.5 flex items-end justify-between">
                  <div>
                    <h2 className="type-section-title text-ink">掌握情况</h2>
                    <p className="mt-0.5 text-[12.5px] text-ink-muted">
                      统计自知识库中的全部实体，不使用估算值
                    </p>
                  </div>
                </div>
                <div className="grid gap-3 sm:grid-cols-2">
                  <ProgressCard
                    title="词汇"
                    value={data.snapshot.mastery.vocabulary.mastered}
                    total={data.snapshot.totals.words}
                    description={`平均掌握度 ${data.snapshot.vocabularyAccuracy}%`}
                    meta={`${data.snapshot.totals.words} 个词条`}
                    segments={[
                      {
                        label: "已掌握",
                        value: data.snapshot.mastery.vocabulary.mastered,
                        color: "var(--success)",
                      },
                      {
                        label: "复习中",
                        value: data.snapshot.mastery.vocabulary.reviewing,
                        color: "var(--brand)",
                      },
                      {
                        label: "学习中",
                        value: data.snapshot.mastery.vocabulary.learning,
                        color: "var(--ember)",
                      },
                      {
                        label: "未学习",
                        value: data.snapshot.mastery.vocabulary.new,
                        color: "var(--line-strong)",
                      },
                    ]}
                  />
                  <ProgressCard
                    title="语法"
                    value={data.snapshot.mastery.grammar.mastered}
                    total={data.snapshot.totals.grammarPoints}
                    description={`平均掌握度 ${data.snapshot.grammarAccuracy}%`}
                    meta={`${data.snapshot.totals.grammarPoints} 个语法点`}
                    segments={[
                      {
                        label: "已掌握",
                        value: data.snapshot.mastery.grammar.mastered,
                        color: "var(--success)",
                      },
                      {
                        label: "复习中",
                        value: data.snapshot.mastery.grammar.reviewing,
                        color: "var(--brand)",
                      },
                      {
                        label: "学习中",
                        value: data.snapshot.mastery.grammar.learning,
                        color: "var(--ember)",
                      },
                      {
                        label: "未学习",
                        value: data.snapshot.mastery.grammar.new,
                        color: "var(--line-strong)",
                      },
                    ]}
                  />
                </div>
              </section>

              <RecentRecords records={data.recentRecords} />
            </div>

            <aside className="min-w-0 space-y-6">
              <ReviewPanel
                data={data}
                onOpenReview={() => navigate("/review")}
                onOpenKnowledge={(id) => navigate(`/knowledge?id=${id}`)}
              />
              <AiNudge
                data={data}
                onApply={() => navigate("/planner")}
              />
              <QuickStart
                options={data.quickStart}
                onStart={(option) =>
                  navigate(trainingRoute({ kind: option.kind, track: option.track }))
                }
              />
            </aside>
          </div>
        </>
      )}
    </PageContainer>
  );
}
