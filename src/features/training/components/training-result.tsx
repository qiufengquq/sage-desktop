import { ArrowRight, RotateCcw, Target, Timer, Trophy } from "lucide-react";
import type { ExerciseResult, TrainingSession } from "@/types";
import { formatDuration } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { RadialGauge } from "@/components/charts/radial-gauge";
import { Badge } from "@/components/ui/badge";
import { MasteryBar } from "@/components/common/mastery-bar";

interface TrainingResultProps {
  result: ExerciseResult;
  session: TrainingSession;
  /** 同一题型再来一组（会重新取题） */
  onRestart: () => void;
  /** 回到训练列表 */
  onExit: () => void;
  /** 跳到知识库对应条目 */
  onReviewWeak?: () => void;
  /** 继续同一 track 的下一个训练入口 */
  onContinue?: () => void;
}

const KIND_LABEL: Record<string, string> = {
  "word-match": "单词连线",
  "multiple-choice": "词义选择",
  spelling: "拼写",
  dictation: "听写",
  cloze: "句子填空",
  translate: "中英互译",
  "sentence-order": "句子排序",
  "error-correction": "语法纠错",
  rewrite: "句子改写",
  shadowing: "跟读",
  roleplay: "情景对话",
};

export function TrainingResultView({
  result,
  session,
  onRestart,
  onExit,
  onReviewWeak,
  onContinue,
}: TrainingResultProps) {
  const verdict =
    result.accuracy >= 85
      ? "这组题已经稳了，可以把难度往上提一档。"
      : result.accuracy >= 65
        ? "基本掌握，仍有 2–3 个点需要复习。"
        : "建议先回到知识库看清规则，再做一遍。";

  return (
    <div className="space-y-5">
      <div className="rounded-md border border-line bg-surface p-5">
        <div className="flex flex-wrap items-center justify-between gap-5">
          <div className="flex items-center gap-4">
            <RadialGauge value={result.accuracy} size={78} thickness={6} label="正确率" />
            <div>
              <h2 className="font-display text-[22px] leading-tight text-ink">
                {result.accuracy >= 85 ? "完成得很干净" : result.accuracy >= 65 ? "训练完成" : "需要再练一轮"}
              </h2>
              <p className="mt-1 max-w-[46ch] text-2xs leading-relaxed text-ink-muted">{verdict}</p>
            </div>
          </div>
          <div className="flex items-center gap-5">
            <Stat icon={Trophy} label="获得经验" value={`+${result.xp}`} tone="ember" />
            <Stat icon={Target} label="得分" value={`${result.score}/${result.maxScore}`} />
            <Stat icon={Timer} label="用时" value={formatDuration(result.durationMs)} />
          </div>
        </div>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <section className="rounded-md border border-line bg-surface p-4">
          <h3 className="text-[13.5px] font-medium text-ink">分项表现</h3>
          <div className="mt-3 space-y-2.5">
            {result.perKind.map((kind) => (
              <div key={kind.kind} className="flex items-center gap-3">
                <span className="w-20 shrink-0 text-2xs text-ink-muted">
                  {KIND_LABEL[kind.kind] ?? kind.kind}
                </span>
                <MasteryBar
                  value={kind.total === 0 ? 0 : (kind.correct / kind.total) * 100}
                  showValue={false}
                  className="flex-1"
                />
                <span className="w-10 shrink-0 text-right font-mono text-2xs text-ink-soft">
                  {kind.correct}/{kind.total}
                </span>
              </div>
            ))}
          </div>
        </section>

        <section className="rounded-md border border-line bg-surface p-4">
          <h3 className="text-[13.5px] font-medium text-ink">需要复习的知识点</h3>
          {result.weakEntityIds.length === 0 ? (
            <p className="mt-3 text-2xs text-ink-muted">
              这组训练没有触发薄弱项。可以把难度调高一档，或去做下一组。
            </p>
          ) : (
            <>
              <div className="mt-3 flex flex-wrap gap-1.5">
                {result.weakEntityIds.map((id) => (
                  <Badge key={id} variant="warning" className="font-mono">
                    {session.exercises
                      .flatMap((e) => e.entityIds)
                      .includes(id)
                      ? id.replace(/^w_|^g_|^s_/, "")
                      : id}
                  </Badge>
                ))}
              </div>
              <p className="mt-3 text-2xs leading-relaxed text-ink-muted">
                这些条目已按本次表现更新掌握度，并写入复习队列，下一次复习时间已按记忆强度重新安排。
              </p>
              {onReviewWeak && (
                <Button variant="ghost" size="sm" className="mt-2 px-0 text-brand" onClick={onReviewWeak}>
                  查看薄弱知识点
                  <ArrowRight />
                </Button>
              )}
            </>
          )}
        </section>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" onClick={onRestart}>
          <RotateCcw />
          再练一组
        </Button>
        <Button variant="outline" onClick={onExit}>
          返回训练列表
        </Button>
        <Button onClick={onContinue ?? onExit}>
          继续学习
          <ArrowRight />
        </Button>
      </div>
    </div>
  );
}

function Stat({
  icon: Icon,
  label,
  value,
  tone,
}: {
  icon: typeof Trophy;
  label: string;
  value: string;
  tone?: "ember";
}) {
  return (
    <div>
      <div className="flex items-center gap-1 text-2xs text-ink-muted">
        <Icon className="size-3" />
        {label}
      </div>
      <div
        className={`type-numeral mt-0.5 text-base ${tone === "ember" ? "text-ember" : "text-ink"}`}
      >
        {value}
      </div>
    </div>
  );
}
