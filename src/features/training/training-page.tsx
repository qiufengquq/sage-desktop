import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { Dumbbell, Loader2, Mic, SpellCheck, SquareStack } from "lucide-react";
import { toast } from "sonner";
import type { ApiError, ExerciseAnswer, ExerciseKind, ExerciseResult, TrainingSession, TrainingTrack } from "@/types";
import { trainingService } from "@/services/api";
import { useQuery } from "@/hooks/use-query";
import { PageContainer } from "@/components/common/page-container";
import { PageHeader } from "@/components/common/page-header";
import { ErrorState, LoadingState } from "@/components/common/states";
import { ExerciseCard } from "@/components/common/exercise-card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { TrainingSessionRunner } from "./components/training-session";
import { TrainingResultView } from "./components/training-result";

const TRACKS: { id: TrainingTrack; label: string; icon: typeof Mic; description: string }[] = [
  {
    id: "vocabulary",
    label: "词汇训练",
    icon: SpellCheck,
    description: "连线、选择、拼写、听写、填空与互译",
  },
  {
    id: "grammar",
    label: "语法与句子",
    icon: SquareStack,
    description: "语法选择、排序、纠错、填空、翻译与改写",
  },
  {
    id: "speaking",
    label: "口语训练",
    icon: Mic,
    description: "跟读、情景对话与语音录入",
  },
];

type Stage = "browse" | "running" | "result";

export function TrainingPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const navigate = useNavigate();
  const [stage, setStage] = useState<Stage>("browse");
  const [activeTrack, setActiveTrack] = useState<TrainingTrack>(
    (searchParams.get("track") as TrainingTrack | null) ?? "vocabulary",
  );
  const [session, setSession] = useState<TrainingSession | null>(null);
  const [result, setResult] = useState<ExerciseResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [submitBusy, setSubmitBusy] = useState(false);
  const [actionError, setActionError] = useState<ApiError | null>(null);

  const { data: options, error, reload, initial } = useQuery(
    (opts) => trainingService.getTrainingOptions(undefined, opts),
    [],
  );

  const start = useCallback(async (kind: ExerciseKind, track: TrainingTrack, title?: string) => {
    setBusy(true);
    setActionError(null);
    const response = await trainingService.startSession({ kind, track, title });
    setBusy(false);
    if (response.ok) {
      setSession(response.data);
      setResult(null);
      setStage("running");
    } else {
      setActionError(response.error);
    }
  }, []);

  // 支持从首页 / AI 规划直接跳转到某个训练并自动开始
  useEffect(() => {
    const kind = searchParams.get("kind") as ExerciseKind | null;
    const track = (searchParams.get("track") as TrainingTrack | null) ?? "vocabulary";
    if (searchParams.get("start") === "1" && kind && !session && !busy) {
      void start(kind, track);
      setSearchParams({}, { replace: true });
    }
  }, [searchParams, session, busy, start, setSearchParams]);

  const submit = useCallback(
    async (answers: ExerciseAnswer[]) => {
      if (!session) return;
      setSubmitBusy(true);
      const response = await trainingService.submitSession({ session, answers });
      setSubmitBusy(false);
      if (response.ok) {
        setResult(response.data);
        setStage("result");
        toast.success(`训练完成，获得 ${response.data.xp} XP`, {
          description: `正确率 ${response.data.accuracy}%`,
        });
      } else {
        setActionError(response.error);
      }
    },
    [session],
  );

  const exitToBrowse = () => {
    setStage("browse");
    setSession(null);
    setResult(null);
    setSearchParams({}, { replace: true });
  };

  const grouped = useMemo(() => {
    const list = options ?? [];
    return TRACKS.map((track) => ({
      ...track,
      items: list.filter((o) => o.track === track.id),
    }));
  }, [options]);

  if (stage === "running" && session) {
    return (
      <PageContainer className="max-w-[880px]">
        <TrainingSessionRunner
          session={session}
          submitting={submitBusy}
          onSubmit={submit}
          onExit={exitToBrowse}
        />
        {actionError && <ErrorState error={actionError} className="mt-4" />}
      </PageContainer>
    );
  }

  if (stage === "result" && result && session) {
    const weakEntityId = result.weakEntityIds[0];
    return (
      <PageContainer className="max-w-[900px]">
        <TrainingResultView
          result={result}
          session={session}
          onRestart={() => start(session.kind, session.track, session.title)}
          onExit={exitToBrowse}
          onReviewWeak={
            weakEntityId
              ? () => navigate(`/knowledge?id=${encodeURIComponent(weakEntityId)}`)
              : undefined
          }
          onContinue={() => {
            setActiveTrack(session.track);
            exitToBrowse();
          }}
        />
      </PageContainer>
    );
  }

  return (
    <PageContainer>
      <PageHeader
        title="训练"
        description="每一类训练都是独立组件，但共用同一套题目模型、判分规则与结果接口。选择一项开始，或从首页的任务直接进入。"
        meta={
          <>
            <span className="flex items-center gap-1 text-2xs text-ink-muted">
              <Dumbbell className="size-3" />
              共 {options?.length ?? 0} 种训练方式
            </span>
            <span className="text-2xs text-ink-muted">结果会写回知识库的掌握度</span>
          </>
        }
      />

      {initial && <LoadingState variant="cards" rows={6} />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {actionError && <ErrorState error={actionError} className="mb-4" />}
      {busy && (
        <div className="mb-4 flex items-center gap-2 rounded-md border border-line bg-surface px-3 py-2 text-2xs text-ink-muted">
          <Loader2 className="size-3 animate-spin" />
          正在准备题目…
        </div>
      )}

      {options && (
        <Tabs value={activeTrack} onValueChange={(value) => setActiveTrack(value as TrainingTrack)}>
          <TabsList>
            {TRACKS.map((track) => {
              const Icon = track.icon;
              return (
                <TabsTrigger key={track.id} value={track.id}>
                  <Icon />
                  {track.label}
                </TabsTrigger>
              );
            })}
          </TabsList>

          {grouped.map((track) => (
            <TabsContent key={track.id} value={track.id}>
              <p className="mb-3 text-2xs text-ink-muted">{track.description}</p>
              <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
                {track.items.map((option) => (
                  <ExerciseCard
                    key={option.id}
                    option={option}
                    onStart={() => start(option.kind, option.track, option.title)}
                  />
                ))}
              </div>
            </TabsContent>
          ))}
        </Tabs>
      )}
    </PageContainer>
  );
}
