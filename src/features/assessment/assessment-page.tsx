import { useState } from "react";
import { useNavigate } from "react-router";
import { ArrowRight, ClipboardCheck, Loader2, Mic, SpellCheck, SquareStack } from "lucide-react";
import type { AssessmentModule, AssessmentModuleId, AssessmentOutcome, ExerciseAnswer } from "@/types";
import { assessmentService } from "@/services/api";
import { useQuery } from "@/hooks/use-query";
import { formatRelativeTime } from "@/lib/date";
import { trainingRoute } from "@/lib/plan-target";
import { PageContainer } from "@/components/common/page-container";
import { PageHeader } from "@/components/common/page-header";
import { ErrorState, LoadingState } from "@/components/common/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { AssessmentRunner } from "./components/assessment-runner";
import { AssessmentOutcomeView } from "./components/assessment-outcome";

const MODULE_ICON = {
  vocabulary: SpellCheck,
  grammar: SquareStack,
  speaking: Mic,
} as const;

type Stage = "browse" | "running" | "outcome";

export function AssessmentPage() {
  const navigate = useNavigate();
  const { data: modules, error, reload, initial } = useQuery(
    (options) => assessmentService.getAssessmentModules(options),
    [],
  );

  const [stage, setStage] = useState<Stage>("browse");
  const [activeId, setActiveId] = useState<AssessmentModuleId>("vocabulary");
  const [outcome, setOutcome] = useState<AssessmentOutcome | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [submissionId, setSubmissionId] = useState<string | null>(null);

  const activeModule = modules?.find((m) => m.id === activeId) ?? modules?.[0];

  const submit = async (answers: ExerciseAnswer[]) => {
    if (!activeModule) return;
    setSubmitting(true);
    setSubmitError(null);
    // submissionId 是幂等键，必须始终有值：未开始时现生成并落回状态
    const requestId = submissionId ?? crypto.randomUUID();
    if (!submissionId) setSubmissionId(requestId);
    const response = await assessmentService.submitAssessment({
      moduleId: activeModule.id,
      answers,
      submissionId: requestId,
    });
    setSubmitting(false);
    if (response.ok) {
      setOutcome(response.data);
      setStage("outcome");
    } else {
      setSubmitError(response.error.message);
    }
  };

  if (stage === "running" && activeModule) {
    return (
      <PageContainer className="max-w-[860px]">
        <AssessmentRunner
          module={activeModule}
          submitting={submitting}
          onSubmit={submit}
          onExit={() => setStage("browse")}
        />
        {submitError && <p className="mt-3 text-2xs text-danger">{submitError}</p>}
      </PageContainer>
    );
  }

  if (stage === "outcome" && outcome && activeModule) {
    return (
      <PageContainer className="max-w-[900px]">
        <AssessmentOutcomeView
          outcome={outcome}
          moduleTitle={activeModule.title}
          onRetry={() => {
            setOutcome(null);
            setStage("running");
          }}
          onExit={() => {
            setOutcome(null);
            setStage("browse");
          }}
          onTrain={() => {
            const followUp = outcome.followUp;
            setOutcome(null);
            setStage("browse");
            if (followUp) {
              navigate(trainingRoute({ kind: followUp.kind, track: followUp.track }));
            }
          }}
        />
      </PageContainer>
    );
  }

  return (
    <PageContainer>
      <PageHeader
        title="测评"
        description="三个模块分别测量词汇广度、语法准确度与口语表达。结果会保存到本机，并立即更新能力画像与学习计划。"
        meta={
          <>
            <span className="flex items-center gap-1 text-2xs text-ink-muted">
              <ClipboardCheck className="size-3" />
              共 15 个考察维度
            </span>
            <span className="text-2xs text-ink-muted">约 36 分钟 · 可分次完成</span>
          </>
        }
      />

      {initial && <LoadingState variant="cards" rows={3} />}
      {error && <ErrorState error={error} onRetry={reload} />}

      {modules && (
        <Tabs
          value={activeId}
          onValueChange={(value) => setActiveId(value as AssessmentModuleId)}
        >
          <TabsList>
            {modules.map((module) => {
              const Icon = MODULE_ICON[module.id];
              return (
                <TabsTrigger key={module.id} value={module.id}>
                  <Icon />
                  {module.title}
                </TabsTrigger>
              );
            })}
          </TabsList>

          {modules.map((module) => (
            <TabsContent key={module.id} value={module.id}>
              <ModuleOverview
                module={module}
                onStart={() => {
                  setActiveId(module.id);
                  setSubmissionId(crypto.randomUUID());
                  setStage("running");
                }}
              />
            </TabsContent>
          ))}
        </Tabs>
      )}
    </PageContainer>
  );
}

function ModuleOverview({
  module,
  onStart,
}: {
  module: AssessmentModule;
  onStart: () => void;
}) {
  const itemCount = module.sections.reduce((acc, section) => acc + section.items.length, 0);
  const Icon = MODULE_ICON[module.id];

  return (
    <div className="grid gap-7 xl:grid-cols-[minmax(0,1fr)_19rem] xl:gap-9">
      <div className="min-w-0">
        <div className="flex items-start gap-3">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-sm bg-brand-tint text-brand">
            <Icon className="size-4.5" />
          </span>
          <div>
            <h2 className="font-display text-[23px] leading-tight text-ink">{module.title}</h2>
            <p className="text-2xs text-ink-muted">{module.subtitle}</p>
          </div>
        </div>

        <p className="mt-3 max-w-[62ch] text-[13px] leading-relaxed text-ink-soft">
          {module.description}
        </p>

        <ol className="mt-5 divide-y divide-line border-y border-line">
          {module.sections.map((section, index) => (
            <li key={section.id} className="flex items-start gap-3 py-3">
              <span className="mt-0.5 font-mono text-2xs text-ink-muted">
                {String(index + 1).padStart(2, "0")}
              </span>
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-[13.5px] font-medium text-ink">{section.title}</span>
                  <Badge variant="outline">{section.skill}</Badge>
                  <span className="font-mono text-2xs text-ink-muted">
                    {section.items.length > 0 ? `${section.items.length} 题` : "评估汇总"}
                  </span>
                </div>
                <p className="mt-0.5 text-2xs leading-relaxed text-ink-muted">
                  {section.description}
                </p>
              </div>
            </li>
          ))}
        </ol>
      </div>

      <aside className="min-w-0">
        <div className="rounded-md border border-line bg-surface p-4">
          <div className="flex items-baseline gap-1.5">
            <span className="type-numeral text-2xl leading-none text-ink">{module.minutes}</span>
            <span className="text-2xs text-ink-muted">分钟</span>
          </div>
          <dl className="mt-3 space-y-1.5 text-2xs">
            <div className="flex justify-between">
              <dt className="text-ink-muted">分项</dt>
              <dd className="font-mono text-ink-soft">{module.sections.length}</dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-ink-muted">题目</dt>
              <dd className="font-mono text-ink-soft">{itemCount}</dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-ink-muted">上次成绩</dt>
              <dd className="font-mono text-ink-soft">
                {module.lastAttempt ? `${module.lastAttempt.score} 分` : "未测试"}
              </dd>
            </div>
            {module.lastAttempt && (
              <div className="flex justify-between">
                <dt className="text-ink-muted">时间</dt>
                <dd className="text-ink-soft">{formatRelativeTime(module.lastAttempt.at)}</dd>
              </div>
            )}
          </dl>

          <Button className="mt-4 w-full" onClick={onStart}>
            开始{module.title}
            <ArrowRight />
          </Button>
          <p className="mt-2 text-2xs leading-relaxed text-ink-muted">
            中途退出会丢弃本次作答；成绩保存在本机，可随时复测。
          </p>
        </div>

        <div className="mt-3 rounded-md border border-line bg-raised/60 p-3.5">
          <div className="flex items-center gap-1.5 text-2xs text-ink-muted">
            <Loader2 className="size-3" />
            语音部分说明
          </div>
          <p className="mt-1 text-2xs leading-relaxed text-ink-muted">
            口语部分目前用录音 + 文本匹配评分，尚未接入语音识别与发音评测，分数仅供参考。
          </p>
        </div>
      </aside>
    </div>
  );
}
