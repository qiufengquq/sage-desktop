import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowLeft, ArrowRight, Check, Clock, Loader2, X } from "lucide-react";
import type { AnswerCheck, Exercise, ExerciseAnswer, TrainingSession } from "@/types";
import { formatDuration } from "@/lib/format";
import { cn } from "@/lib/utils";
import { trainingService } from "@/services/api";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ConfirmDialog } from "@/components/common/confirm-dialog";
import { ExerciseView } from "./exercise-view";
import type { AnswerValue } from "./answer-controls";

interface TrainingSessionRunnerProps {
  session: TrainingSession;
  submitting?: boolean;
  onSubmit: (answers: ExerciseAnswer[]) => void;
  onExit: () => void;
}

function hasAnswer(value: AnswerValue) {
  if (Array.isArray(value)) return value.length > 0;
  if (typeof value === "object" && value !== null) {
    return Object.values(value).some((v) => String(v).trim().length > 0);
  }
  return String(value ?? "").trim().length > 0;
}

export function TrainingSessionRunner({
  session,
  submitting = false,
  onSubmit,
  onExit,
}: TrainingSessionRunnerProps) {
  const [index, setIndex] = useState(0);
  const [value, setValue] = useState<AnswerValue>("");
  const [revealed, setRevealed] = useState(false);
  const [answers, setAnswers] = useState<ExerciseAnswer[]>([]);
  const [elapsed, setElapsed] = useState(0);
  const [confirmExit, setConfirmExit] = useState(false);
  const [check, setCheck] = useState<AnswerCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState<string | null>(null);
  const questionStart = useRef(Date.now());
  const startedAt = useRef(Date.now());

  const exercise = session.exercises[index] as Exercise | undefined;
  const total = session.exercises.length;
  const isLast = index === total - 1;

  useEffect(() => {
    const timer = setInterval(() => setElapsed(Date.now() - startedAt.current), 1000);
    return () => clearInterval(timer);
  }, []);

  /**
   * 单题校验改为向服务端求证：判分规则只在 Rust 侧（mock 模式用同一套本地规则）。
   * 页面拿到的仍是 `{correct, partial}`，所以渲染逻辑不变。
   */
  const submit = useCallback(async () => {
    if (!exercise || checking) return;
    setChecking(true);
    setCheckError(null);

    const response = await trainingService.checkAnswer({
      sessionId: session.id,
      exerciseId: exercise.id,
      value,
    });

    setChecking(false);
    if (!response.ok) {
      setCheckError(response.error.message);
      return;
    }

    const result = response.data;
    setCheck(result);
    setAnswers((prev) => [
      ...prev.filter((a) => a.exerciseId !== exercise.id),
      {
        exerciseId: exercise.id,
        value,
        correct: result.correct,
        partialScore: result.partialScore,
        elapsedMs: Date.now() - questionStart.current,
      },
    ]);
    setRevealed(true);
  }, [exercise, value, checking, session.id]);

  const next = useCallback(() => {
    if (isLast) {
      onSubmit(answers);
      return;
    }
    setIndex((i) => i + 1);
    setValue("");
    setRevealed(false);
    setCheck(null);
    setCheckError(null);
    questionStart.current = Date.now();
  }, [answers, isLast, onSubmit]);

  if (!exercise) {
    return (
      <div className="rounded-md border border-line bg-surface p-8 text-center text-sm text-ink-muted">
        这组训练还没有题目。
        <Button variant="outline" size="sm" className="mt-3" onClick={onExit}>
          返回训练列表
        </Button>
      </div>
    );
  }

  const canSubmit = hasAnswer(value);

  const requestExit = () => {
    if (answers.length > 0) {
      setConfirmExit(true);
      return;
    }
    onExit();
  };

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="ghost" size="sm" onClick={requestExit} className="text-ink-muted">
          <ArrowLeft />
          退出
        </Button>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-medium text-ink">{session.title}</div>
          <div className="mt-1 flex items-center gap-2">
            <div className="h-1 w-32 overflow-hidden rounded-full bg-sunken">
              <span
                className="block h-full rounded-full bg-brand transition-[width] duration-300"
                style={{ width: `${((index + (revealed ? 1 : 0)) / total) * 100}%` }}
              />
            </div>
            <span className="font-mono text-2xs text-ink-muted">
              {index + 1} / {total}
            </span>
          </div>
        </div>
        <span className="flex items-center gap-1 font-mono text-2xs text-ink-muted">
          <Clock className="size-3" />
          {formatDuration(elapsed)}
        </span>
      </div>

      <div className="rounded-md border border-line bg-surface p-5">
        <ExerciseView
          exercise={exercise}
          value={value}
          onChange={setValue}
          revealed={revealed}
          check={check}
        />

        <div className="mt-5 flex flex-wrap items-center justify-between gap-3 border-t border-line pt-4">
          <div className="flex items-center gap-2">
            {revealed && check && (
              <Badge
                variant={check.correct ? "success" : check.partialScore > 0.5 ? "warning" : "danger"}
              >
                {check.correct ? (
                  <>
                    <Check className="size-3" />
                    回答正确
                  </>
                ) : check.partialScore > 0 ? (
                  <>
                    <X className="size-3" />
                    部分正确（{Math.round(check.partialScore * 100)}%）
                  </>
                ) : (
                  <>
                    <X className="size-3" />
                    需要复习
                  </>
                )}
              </Badge>
            )}
            {!revealed && (
              <span className="text-2xs text-ink-muted">
                预计 {exercise.xp} XP · {exercise.tags[0] ?? "综合"}
              </span>
            )}
          </div>

          {revealed ? (
            <Button onClick={next} disabled={submitting}>
              {submitting && <Loader2 className="animate-spin" />}
              {isLast ? "查看训练结果" : "下一题"}
              {!submitting && <ArrowRight />}
            </Button>
          ) : (
            <div className="flex items-center gap-3">
              {checkError && (
                <span className="text-2xs text-danger">校验失败：{checkError}</span>
              )}
              <Button
                onClick={submit}
                disabled={!canSubmit || checking}
                className={cn((!canSubmit || checking) && "opacity-60")}
              >
                {checking && <Loader2 className="animate-spin" />}
                提交答案
                {!checking && <ArrowRight />}
              </Button>
            </div>
          )}
        </div>
      </div>

      <ConfirmDialog
        open={confirmExit}
        onOpenChange={setConfirmExit}
        title="退出本次训练？"
        description={`已作答 ${answers.length} 道题。退出后本次作答不会保存，也不会更新掌握度。`}
        confirmLabel="退出训练"
        cancelLabel="继续训练"
        destructive
        onConfirm={onExit}
      />
    </div>
  );
}
