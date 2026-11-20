import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { ArrowRight, RefreshCw, Sparkles } from "lucide-react";
import { toast } from "sonner";
import type { PlanAdvice, PlanItem, StudyPlan } from "@/types";
import { plannerService } from "@/services/api";
import { planActionRoute, trainingRoute } from "@/lib/plan-target";
import { useQuery } from "@/hooks/use-query";
import { formatRelativeTime } from "@/lib/date";
import { Button } from "@/components/ui/button";
import { RadialGauge } from "@/components/charts/radial-gauge";
import { PageContainer } from "@/components/common/page-container";
import { PageHeader } from "@/components/common/page-header";
import { ErrorState, LoadingState } from "@/components/common/states";
import { AbilityProfilePanel } from "./components/ability-profile";
import {
  AdviceList,
  DailyPlan,
  DifficultyControl,
  WeeklyGoals,
} from "./components/planner-panels";

export function PlannerPage() {
  const navigate = useNavigate();
  const { data, error, reload, initial } = useQuery(
    (options) => plannerService.getPlan(options),
    [],
  );

  const [plan, setPlan] = useState<StudyPlan | null>(null);
  const [pendingItemId, setPendingItemId] = useState<string | null>(null);
  const [difficulty, setDifficulty] = useState(3);
  const [applying, setApplying] = useState(false);
  const [regenerating, setRegenerating] = useState(false);

  useEffect(() => {
    if (data) {
      setPlan(data);
      setDifficulty(data.difficulty.current);
    }
  }, [data]);

  const toggleItem = async (item: PlanItem) => {
    setPendingItemId(item.id);
    const response = await plannerService.togglePlanItem(item.id);
    setPendingItemId(null);
    if (!response.ok || !plan) return;
    const items = response.data;
    const done = items.filter((i) => i.done).length;
    setPlan({
      ...plan,
      today: { ...plan.today, items },
      completion: {
        done,
        total: items.length,
        rate: Math.round((done / Math.max(1, items.length)) * 100),
      },
    });
  };

  const applyDifficulty = async () => {
    setApplying(true);
    const response = await plannerService.applyDifficulty(difficulty);
    setApplying(false);
    if (response.ok) {
      setPlan(response.data);
      toast.success(`难度已调整为 ${difficulty.toFixed(1)}`, {
        description: "下一组训练会按新难度出题。",
      });
    }
  };

  const regenerate = async () => {
    setRegenerating(true);
    const response = await plannerService.regeneratePlan();
    setRegenerating(false);
    if (response.ok) {
      setPlan(response.data);
      setDifficulty(response.data.difficulty.current);
      toast.success("已重新生成今日计划", {
        description: "依据最近 30 天的学习记录与最新测评结果。",
      });
    }
  };

  const startItem = (item: PlanItem) => navigate(planActionRoute(item.action));

  const startAdvice = (advice: PlanAdvice) =>
    navigate(
      trainingRoute({ kind: advice.action.trainingKind, track: advice.action.track }),
    );

  if (initial) {
    return (
      <PageContainer>
        <PageHeader title="学习规划" description="正在读取你的能力画像与今日计划…" />
        <LoadingState variant="cards" rows={3} />
      </PageContainer>
    );
  }

  if (error || !plan) {
    return (
      <PageContainer>
        <PageHeader title="学习规划" />
        {error ? (
          <ErrorState error={error} onRetry={reload} />
        ) : (
          <LoadingState variant="text" />
        )}
      </PageContainer>
    );
  }

  const nextItem = plan.today.items.find((item) => !item.done);

  return (
    <PageContainer>
      <PageHeader
        title="学习规划"
        description="计划由确定性规则引擎按能力画像、到期复习量、最近正确率与每日时长生成，每一条建议都能追溯到具体数据；本阶段不调用大模型。"
        meta={
          <>
            <span className="flex items-center gap-1 text-2xs text-ink-muted">
              <Sparkles className="size-3" />
              {plan.model}
            </span>
            <span className="text-2xs text-ink-muted">
              生成于 {formatRelativeTime(plan.generatedAt)}
            </span>
          </>
        }
        actions={
          <>
            <Button variant="outline" onClick={regenerate} disabled={regenerating}>
              <RefreshCw className={regenerating ? "animate-spin" : undefined} />
              重新生成
            </Button>
            <Button disabled={!nextItem} onClick={() => nextItem && startItem(nextItem)}>
              开始今日计划
              <ArrowRight />
            </Button>
          </>
        }
      />

      <AbilityProfilePanel
        profile={plan.profile}
        onGoAssessment={() => navigate("/assessment")}
      />

      <div className="mt-7 grid grid-cols-1 gap-7 xl:grid-cols-[minmax(0,1fr)_20rem] xl:gap-8">
        <div className="min-w-0 space-y-7">
          <DailyPlan
            items={plan.today.items}
            pendingItemId={pendingItemId}
            onToggle={toggleItem}
            onStart={startItem}
          />
          <AdviceList advice={plan.advice} onApply={startAdvice} />
        </div>

        <aside className="min-w-0 space-y-5">
          <section className="rounded-md border border-line bg-surface p-4">
            <div className="flex items-center justify-between">
              <div>
                <h3 className="text-[13.5px] font-medium text-ink">计划完成情况</h3>
                <p className="mt-0.5 text-2xs text-ink-muted">
                  今日 {plan.completion.done}/{plan.completion.total} 项
                </p>
              </div>
              <RadialGauge
                value={plan.completion.rate}
                size={62}
                thickness={5}
                label="完成率"
              />
            </div>
            <div className="mt-3 space-y-1.5">
              <Row label="今日重点" value={plan.today.focus} />
              <Row label="计划时长" value={`${plan.today.minutes} 分钟`} />
              <Row
                label="难度"
                value={`${plan.difficulty.current.toFixed(1)} / 5.0`}
              />
            </div>
          </section>

          <WeeklyGoals goals={plan.weeklyGoals} />

          <DifficultyControl
            difficulty={plan.difficulty}
            value={difficulty}
            onChange={setDifficulty}
            onApply={applyDifficulty}
            busy={applying}
          />
        </aside>
      </div>
    </PageContainer>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <span className="shrink-0 text-2xs text-ink-muted">{label}</span>
      <span className="truncate text-right text-2xs text-ink-soft">{value}</span>
    </div>
  );
}
