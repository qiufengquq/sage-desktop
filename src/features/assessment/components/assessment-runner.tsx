import { useCallback, useMemo, useState } from "react";
import { ArrowLeft, ArrowRight, Check, Loader2, X } from "lucide-react";
import type { AnswerCheck, AssessmentModule, ExerciseAnswer } from "@/types";
import { assessmentService } from "@/services/api";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/common/confirm-dialog";
import { ExerciseView } from "@/features/training/components/exercise-view";
import type { AnswerValue } from "@/features/training/components/answer-controls";

interface AssessmentRunnerProps {
  module: AssessmentModule;
  submitting?: boolean;
  onSubmit: (answers: ExerciseAnswer[]) => void;
  onExit: () => void;
}

export function AssessmentRunner({
  module,
  submitting = false,
  onSubmit,
  onExit,
}: AssessmentRunnerProps) {
  const steps = useMemo(
    () =>
      module.sections.flatMap((section) =>
        section.items.map((item) => ({ section, item })),
      ),
    [module],
  );

  const [index, setIndex] = useState(0);
  const [value, setValue] = useState<AnswerValue>("");
  const [revealed, setRevealed] = useState(false);
  const [answers, setAnswers] = useState<ExerciseAnswer[]>([]);
  const [startedAt] = useState(() => Date.now());
  const [confirmExit, setConfirmExit] = useState(false);
  const [check, setCheck] = useState<AnswerCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState<string | null>(null);

  const step = steps[index];
  const total = steps.length;
  const isLast = index === total - 1;

  /** 单题校验由服务端裁定（mock 模式用同一套本地规则，产物同形） */
  const submitStep = useCallback(async () => {
    if (!step || checking) return;
    setChecking(true);
    setCheckError(null);

    const response = await assessmentService.checkAnswer({
      moduleId: module.id,
      exerciseId: step.item.id,
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
      ...prev.filter((a) => a.exerciseId !== step.item.id),
      {
        exerciseId: step.item.id,
        value,
        correct: result.correct,
        partialScore: result.partialScore,
        elapsedMs: Date.now() - startedAt,
      },
    ]);
    setRevealed(true);
  }, [step, value, startedAt, checking, module.id]);

  const nextStep = () => {
    if (isLast) {
      onSubmit(answers);
      return;
    }
    setIndex((i) => i + 1);
    setValue("");
    setRevealed(false);
    setCheck(null);
    setCheckError(null);
  };

  if (!step) {
    return (
      <div className="rounded-md border border-line bg-surface p-8 text-center text-sm text-ink-muted">
        这个测试还没有题目。
        <Button variant="outline" size="sm" className="mt-3" onClick={onExit}>
          返回测评
        </Button>
      </div>
    );
  }

  const sectionIndex = module.sections.findIndex((s) => s.id === step.section.id) + 1;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-3">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => (answers.length > 0 ? setConfirmExit(true) : onExit())}
            className="text-ink-muted"
          >
            <ArrowLeft />
            退出测试
          </Button>
          <div>
            <div className="text-[13px] font-medium text-ink">{module.title}</div>
            <div className="text-2xs text-ink-muted">
              分项 {sectionIndex}/{module.sections.length} · {step.section.title}
            </div>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <div className="h-1 w-28 overflow-hidden rounded-full bg-sunken">
            <span
              className="block h-full bg-brand transition-[width] duration-300"
              style={{ width: `${((index + (revealed ? 1 : 0)) / total) * 100}%` }}
            />
          </div>
          <span className="font-mono text-2xs text-ink-muted">
            {index + 1}/{total}
          </span>
        </div>
      </div>

      <div className="flex flex-wrap gap-1.5">
        {module.sections.map((section) => {
          const done = section.items.every((item) =>
            answers.some((a) => a.exerciseId === item.id),
          );
          const isCurrent = section.id === step.section.id;
          return (
            <Badge
              key={section.id}
              variant={done ? "success" : isCurrent ? "brand" : "outline"}
              className="text-2xs"
            >
              {section.title}
            </Badge>
          );
        })}
      </div>

      <div className="rounded-md border border-line bg-surface p-5">
        <ExerciseView
          exercise={step.item}
          value={value}
          onChange={setValue}
          revealed={revealed}
          check={check}
        />

        <div className="mt-5 flex flex-wrap items-center justify-between gap-3 border-t border-line pt-4">
          <div>
            {check && (
              <Badge
                variant={
                  check.correct ? "success" : check.partialScore > 0.5 ? "warning" : "danger"
                }
              >
                {check.correct ? <Check className="size-3" /> : <X className="size-3" />}
                {check.correct
                  ? "正确"
                  : check.partialScore > 0
                    ? `部分正确（${Math.round(check.partialScore * 100)}%）`
                    : "错误"}
              </Badge>
            )}
            {!check && (
              <span className="text-2xs text-ink-muted">
                考察维度：{step.section.skill}
              </span>
            )}
          </div>

          {revealed ? (
            <Button onClick={nextStep} disabled={submitting}>
              {submitting && <Loader2 className="animate-spin" />}
              {isLast ? "查看测评结果" : "下一题"}
              {!submitting && <ArrowRight />}
            </Button>
          ) : (
            <div className="flex items-center gap-3">
              {checkError && <span className="text-2xs text-danger">校验失败：{checkError}</span>}
              <Button
                onClick={submitStep}
                disabled={checking}
                className={cn((!value || checking) && "opacity-60")}
              >
                {checking && <Loader2 className="animate-spin" />}
                确认作答
                {!checking && <ArrowRight />}
              </Button>
            </div>
          )}
        </div>
      </div>

      <ConfirmDialog
        open={confirmExit}
        onOpenChange={setConfirmExit}
        title="退出本次测评？"
        description={`已作答 ${answers.length} 道题。退出后本次作答不会保存，也不会更新能力画像。`}
        confirmLabel="退出测评"
        cancelLabel="继续作答"
        destructive
        onConfirm={onExit}
      />
    </div>
  );
}
