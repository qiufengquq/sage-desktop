import { useEffect, useMemo, useState } from "react";
import { Check, Link2, RotateCcw, Volume2, X } from "lucide-react";
import { cn, normalizeAnswer } from "@/lib/utils";
import type { ExerciseAnswer, ExerciseOption, MatchPair, RoleplayTurn } from "@/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input, Textarea } from "@/components/ui/input";
import { VoiceRecorder } from "@/components/common/voice-recorder";
import { speak } from "@/lib/speech";

export type AnswerValue = ExerciseAnswer["value"];

interface BaseControlProps {
  value: AnswerValue;
  onChange: (value: AnswerValue) => void;
  revealed?: boolean;
}

/* --------------------------------- 选择题 --------------------------------- */
export function ChoiceAnswer({
  options,
  correctLabels,
  value,
  onChange,
  revealed,
}: BaseControlProps & {
  options: ExerciseOption[];
  /** 正确选项文案：后端下发题目不含 correct 标记，只能从判分结果里取 */
  correctLabels?: string[];
}) {
  const chosen = typeof value === "string" ? value : "";
  const letters = ["A", "B", "C", "D", "E"];
  const labels = correctLabels ?? [];

  return (
    <div className="grid gap-2" role="radiogroup" aria-label="选项">
      {options.map((option, index) => {
        const isChosen = chosen === option.id;
        const isCorrectOption = option.correct ?? labels.includes(option.label);
        const showCorrect = revealed && isCorrectOption;
        const showWrong = revealed && isChosen && !isCorrectOption;
        return (
          <button
            key={option.id}
            type="button"
            role="radio"
            aria-checked={isChosen}
            disabled={revealed}
            onClick={() => onChange(option.id)}
            className={cn(
              "flex w-full items-center gap-3 rounded-md border px-3 py-2.5 text-left text-[13.5px] transition-colors",
              "disabled:cursor-default",
              showCorrect
                ? "border-success/50 bg-success/[0.06] text-ink"
                : showWrong
                  ? "border-danger/50 bg-danger/[0.05] text-ink"
                  : isChosen
                    ? "border-brand/55 bg-brand-tint/40 text-ink"
                    : "border-line bg-surface text-ink-soft hover:border-line-strong hover:bg-raised",
            )}
          >
            <span
              className={cn(
                "flex size-5 shrink-0 items-center justify-center rounded-[4px] font-mono text-2xs",
                isChosen || showCorrect
                  ? "bg-brand text-brand-ink"
                  : "bg-sunken text-ink-muted",
                showCorrect && "bg-success",
                showWrong && "bg-danger text-white",
              )}
            >
              {showCorrect ? <Check className="size-3" strokeWidth={3} /> : showWrong ? <X className="size-3" /> : letters[index]}
            </span>
            <span className="min-w-0 flex-1">{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}

/* --------------------------------- 文本题 --------------------------------- */
export function TextAnswer({
  value,
  onChange,
  revealed,
  placeholder = "在这里输入答案",
  multiline = false,
  expected,
  hint,
  suffix,
}: BaseControlProps & {
  placeholder?: string;
  multiline?: boolean;
  expected?: string;
  hint?: string;
  suffix?: React.ReactNode;
}) {
  const text = typeof value === "string" ? value : "";
  const isCorrect = revealed && expected ? normalizeAnswer(text) === normalizeAnswer(expected) : null;

  return (
    <div className="space-y-2">
      <div className="flex items-start gap-2">
        {multiline ? (
          <Textarea
            value={text}
            onChange={(e) => onChange(e.target.value)}
            placeholder={placeholder}
            disabled={revealed}
            rows={3}
            aria-label="答案输入"
            className={cn(
              revealed && isCorrect && "shadow-[inset_0_0_0_1px_var(--success)]",
              revealed && isCorrect === false && "shadow-[inset_0_0_0_1px_var(--danger)]",
            )}
          />
        ) : (
          <Input
            value={text}
            onChange={(e) => onChange(e.target.value)}
            placeholder={placeholder}
            disabled={revealed}
            aria-label="答案输入"
            autoComplete="off"
            spellCheck={false}
            className={cn(
              "h-10",
              revealed && isCorrect && "shadow-[inset_0_0_0_1px_var(--success)]",
              revealed && isCorrect === false && "shadow-[inset_0_0_0_1px_var(--danger)]",
            )}
          />
        )}
        {suffix}
      </div>
      {hint && !revealed && (
        <p className="font-mono text-2xs tracking-wider text-ink-muted">{hint}</p>
      )}
      {revealed && expected && isCorrect === false && (
        <p className="text-2xs text-ink-muted">
          正确答案：<span className="font-mono text-ink">{expected}</span>
        </p>
      )}
    </div>
  );
}

/* --------------------------------- 连线题 --------------------------------- */
export function MatchAnswer({
  lefts,
  rights,
  correctPairs,
  value,
  onChange,
  revealed,
}: BaseControlProps & {
  /** 左列素材（后端下发） */
  lefts?: string[];
  /** 右列素材（后端下发，未配对） */
  rights?: string[];
  /** 正确配对：来自服务端反馈，只有 revealed 后才用于着色 */
  correctPairs?: MatchPair[];
}) {
  const mapping = (typeof value === "object" && !Array.isArray(value) ? value : {}) as Record<
    string,
    string
  >;
  const [activeLeft, setActiveLeft] = useState<string | null>(null);

  const leftItems = useMemo(
    () => (lefts?.length ? lefts : (correctPairs ?? []).map((pair) => pair.left)),
    [lefts, correctPairs],
  );

  const rightItems = useMemo(() => {
    const list = rights?.length ? rights : (correctPairs ?? []).map((pair) => pair.right);
    return [...list].sort((a, b) => a.localeCompare(b, "zh"));
  }, [rights, correctPairs]);

  const truth = useMemo(
    () => new Map((correctPairs ?? []).map((pair) => [pair.left, pair.right])),
    [correctPairs],
  );

  const assign = (left: string, right: string) => {
    const next = { ...mapping };
    const existing = Object.entries(next).find(([, r]) => r === right);
    if (existing) delete next[existing[0]];
    next[left] = right;
    onChange(next);
    setActiveLeft(null);
  };

  const matchedCount = Object.values(mapping).length;

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between text-2xs text-ink-muted">
        <span>先点左边的单词，再点右边的释义</span>
        <span className="font-mono">
          {matchedCount}/{leftItems.length} 已配对
        </span>
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1.5">
          {leftItems.map((left) => {
            const paired = mapping[left];
            const isActive = activeLeft === left;
            const correct = revealed && truth.get(left) === paired;
            return (
              <button
                key={left}
                type="button"
                disabled={revealed}
                onClick={() => setActiveLeft(isActive ? null : left)}
                className={cn(
                  "flex w-full items-center justify-between gap-2 rounded-md border px-2.5 py-2 text-left text-[13px] transition-colors",
                  isActive
                    ? "border-brand bg-brand-tint/50 text-ink"
                    : paired
                      ? "border-line-strong bg-surface text-ink"
                      : "border-line bg-surface text-ink-soft hover:border-line-strong",
                  revealed && correct && "border-success/50 bg-success/[0.06]",
                  revealed && paired && !correct && "border-danger/40 bg-danger/[0.04]",
                )}
              >
                <span className="font-display text-[15px]">{left}</span>
                {paired && (
                  <span className="flex items-center gap-1 text-2xs text-ink-muted">
                    <Link2 className="size-3" />
                    {paired}
                  </span>
                )}
              </button>
            );
          })}
        </div>

        <div className="space-y-1.5">
          {rightItems.map((right) => {
            const used = Object.values(mapping).includes(right);
            return (
              <button
                key={right}
                type="button"
                disabled={revealed || !activeLeft}
                onClick={() => activeLeft && assign(activeLeft, right)}
                className={cn(
                  "w-full rounded-md border px-2.5 py-2 text-left text-[13px] transition-colors",
                  used
                    ? "border-line bg-sunken/60 text-ink-muted"
                    : "border-line bg-surface text-ink-soft",
                  activeLeft && !revealed && "border-line-strong hover:border-brand/60 hover:bg-brand-tint/30",
                  !activeLeft && "opacity-70",
                )}
              >
                {right}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

/* --------------------------------- 排序题 --------------------------------- */
export function OrderAnswer({
  tokens,
  value,
  onChange,
  revealed,
}: BaseControlProps & { tokens: string[] }) {
  const ordered = Array.isArray(value) ? value : [];
  // 词块数量很小，直接计算比缓存更简单，也避免依赖数组每次都是新引用
  const counts = new Map<string, number>();
  ordered.forEach((token) => counts.set(token, (counts.get(token) ?? 0) + 1));
  const remaining = tokens.filter((token) => {
    const used = counts.get(token) ?? 0;
    if (used > 0) {
      counts.set(token, used - 1);
      return false;
    }
    return true;
  });

  return (
    <div className="space-y-3">
      <div
        className={cn(
          "min-h-14 rounded-md border border-dashed px-3 py-2.5",
          ordered.length > 0 ? "border-line-strong bg-raised" : "border-line bg-sunken/40",
        )}
      >
        {ordered.length === 0 ? (
          <span className="text-2xs text-ink-muted">从下方词块中选择，按顺序排列</span>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {ordered.map((token, index) => (
              <button
                key={`${token}-${index}`}
                type="button"
                disabled={revealed}
                onClick={() => onChange(ordered.filter((_, i) => i !== index))}
                className="group inline-flex items-center gap-1 rounded-sm bg-surface px-2 py-1 text-[13px] text-ink shadow-hairline transition-colors hover:text-danger"
              >
                {token}
                <X className="size-2.5 opacity-40 group-hover:opacity-100" />
              </button>
            ))}
          </div>
        )}
      </div>

      <div className="flex flex-wrap gap-1.5">
        {remaining.map((token, index) => (
          <button
            key={`${token}-rest-${index}`}
            type="button"
            disabled={revealed}
            onClick={() => onChange([...ordered, token])}
            className="rounded-sm bg-sunken px-2 py-1 text-[13px] text-ink-soft transition-colors hover:bg-brand-tint hover:text-brand disabled:opacity-50"
          >
            {token}
          </button>
        ))}
      </div>

      {ordered.length > 0 && !revealed && (
        <Button variant="ghost" size="sm" onClick={() => onChange([])} className="text-ink-muted">
          <RotateCcw />
          清空
        </Button>
      )}
    </div>
  );
}

/* --------------------------------- 听写题 --------------------------------- */
export function DictationAnswer({
  audioText,
  value,
  onChange,
  revealed,
  expected,
  hint,
}: BaseControlProps & { audioText: string; expected?: string; hint?: string }) {
  return (
    <div className="space-y-3">
      <Button variant="outline" size="sm" onClick={() => speak(audioText)}>
        <Volume2 />
        播放发音
      </Button>
      <TextAnswer
        value={value}
        onChange={onChange}
        revealed={revealed}
        expected={expected}
        hint={hint}
        placeholder="听发音后写出单词"
      />
    </div>
  );
}

/* --------------------------------- 跟读题 --------------------------------- */
export function ShadowingAnswer({
  text,
  chunks,
  translation,
  value,
  onChange,
  revealed,
  expected,
}: BaseControlProps & {
  text: string;
  chunks: string[];
  translation: string;
  expected?: string;
}) {
  const [recorded, setRecorded] = useState(false);

  useEffect(() => {
    setRecorded(false);
  }, [text]);

  return (
    <div className="space-y-3">
      <div className="rounded-md border border-line bg-raised px-3.5 py-3">
        <div className="flex items-start justify-between gap-3">
          <p className="font-display text-[17px] leading-relaxed text-ink">{text}</p>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => speak(text)}
            aria-label="播放示范朗读"
          >
            <Volume2 />
          </Button>
        </div>
        <div className="mt-2 flex flex-wrap gap-1.5">
          {chunks.map((chunk) => (
            <span
              key={chunk}
              className="rounded-sm bg-brand-tint/60 px-1.5 py-0.5 text-2xs text-brand"
            >
              {chunk}
            </span>
          ))}
        </div>
        <p className="mt-2 text-2xs text-ink-muted">{translation}</p>
      </div>

      <VoiceRecorder onRecorded={() => setRecorded(true)} onClear={() => setRecorded(false)} />

      <div className="space-y-1.5">
        <label className="text-2xs text-ink-muted" htmlFor="shadow-transcript">
          {recorded ? "录音已保存。把你的朗读内容写下来用于比对：" : "也可以直接输入你的朗读内容："}
        </label>
        {recorded && (
          <Badge variant="success">
            <Check className="size-3" />
            已录音
          </Badge>
        )}
        <TextAnswer
          value={value}
          onChange={onChange}
          revealed={revealed}
          expected={expected}
          multiline
          placeholder="Type what you read aloud…"
        />
      </div>
    </div>
  );
}

/* --------------------------------- 情景对话 -------------------------------- */
export function RoleplayAnswer({
  scenario,
  turns,
  expectedById,
  value,
  onChange,
  revealed,
}: BaseControlProps & {
  scenario: string;
  turns: RoleplayTurn[];
  /** 可接受表达：来自服务端判分结果，题目本身不再携带 `expected` */
  expectedById?: Record<string, string[]>;
}) {
  const answers = (typeof value === "object" && !Array.isArray(value) ? value : {}) as Record<
    string,
    string
  >;

  const update = (turnId: string, text: string) => {
    onChange({ ...answers, [turnId]: text });
  };

  return (
    <div className="space-y-3">
      <p className="rounded-sm bg-sunken/60 px-3 py-2 text-2xs leading-relaxed text-ink-soft">
        情景：{scenario}
      </p>
      <div className="space-y-2.5">
        {turns.map((turn) =>
          turn.speaker === "ai" ? (
            <div key={turn.id} className="flex gap-2">
              <span className="mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-full bg-brand-tint text-[10px] font-medium text-brand">
                对方
              </span>
              <div className="min-w-0">
                <p className="text-[13px] leading-relaxed text-ink">{turn.en}</p>
                <p className="text-2xs text-ink-muted">{turn.zh}</p>
              </div>
            </div>
          ) : (
            <div key={turn.id} className="flex flex-col gap-1.5 pl-8">
              <span className="text-2xs text-ink-muted">你的回合 · {turn.zh}</span>
              <Textarea
                value={answers[turn.id] ?? ""}
                onChange={(e) => update(turn.id, e.target.value)}
                disabled={revealed}
                rows={2}
                placeholder="用英文回应…"
                aria-label={`第 ${turn.id} 轮回应`}
              />
              {revealed && (
                <p className="text-2xs text-ink-muted">
                  参考表达：
                  <span className="text-ink-soft">
                    {(expectedById?.[turn.id] ?? turn.expected ?? []).join(" / ") || "—"}
                  </span>
                </p>
              )}
            </div>
          ),
        )}
      </div>
    </div>
  );
}
