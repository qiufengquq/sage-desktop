import { Info } from "lucide-react";
import type { AnswerCheck, Exercise, MatchPair } from "@/types";
import { Badge } from "@/components/ui/badge";
import { DIFFICULTY_LABEL } from "@/types/vocabulary";
import {
  ChoiceAnswer,
  DictationAnswer,
  MatchAnswer,
  OrderAnswer,
  RoleplayAnswer,
  ShadowingAnswer,
  TextAnswer,
  type AnswerValue,
} from "./answer-controls";

interface ExerciseViewProps {
  exercise: Exercise;
  value: AnswerValue;
  onChange: (value: AnswerValue) => void;
  revealed?: boolean;
  /**
   * 服务端返回的判分结果。
   * 后端下发的题目**不含答案**，所以"正确答案/正确配对/参考表达"都必须从这里取。
   */
  check?: AnswerCheck | null;
}

/** 从 AnswerCheck 里取可读答案的文本形式 */
function expectedText(check?: AnswerCheck | null, fallback?: string): string | undefined {
  const value = check?.correctAnswer;
  if (typeof value === "string" && value.length > 0) return value;
  if (Array.isArray(value) && value.every((item) => typeof item === "string")) {
    return (value as string[]).join(" / ");
  }
  return fallback;
}

/** 角色扮演的"可接受表达"来自服务端反馈，题目本身不再携带 */
function expectedByIdOf(check?: AnswerCheck | null): Record<string, string[]> {
  const value = check?.correctAnswer;
  if (!Array.isArray(value)) return {};

  const map: Record<string, string[]> = {};
  for (const item of value) {
    if (item && typeof item === "object" && "id" in item) {
      const { id, expected } = item as { id: unknown; expected?: unknown };
      map[String(id)] = Array.isArray(expected) ? expected.map(String) : [];
    }
  }
  return map;
}

function correctLabelsOf(check?: AnswerCheck | null): string[] {
  const value = check?.correctAnswer;
  if (typeof value === "string") return [value];
  if (Array.isArray(value)) return value.filter((item): item is string => typeof item === "string");
  return [];
}

export function ExerciseView({
  exercise,
  value,
  onChange,
  revealed = false,
  check = null,
}: ExerciseViewProps) {
  return (
    <div className="space-y-4">
      <div>
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant="outline">{DIFFICULTY_LABEL[exercise.difficulty]}</Badge>
          {exercise.tags.map((tag) => (
            <Badge key={tag} variant="default">
              {tag}
            </Badge>
          ))}
        </div>
        <h2 className="mt-2.5 font-display text-[21px] leading-snug text-ink">
          {exercise.prompt.instruction}
        </h2>
        {exercise.prompt.context && (
          <p className="mt-1 text-[12.5px] text-ink-muted">{exercise.prompt.context}</p>
        )}
      </div>

      <div className="min-w-0">{renderControl(exercise, value, onChange, revealed, check)}</div>

      {revealed && (
        <div className="flex gap-2 rounded-sm bg-sunken/60 px-3 py-2.5">
          <Info className="mt-0.5 size-3.5 shrink-0 text-ink-muted" />
          <p className="text-2xs leading-relaxed text-ink-soft">{exercise.explanation}</p>
        </div>
      )}
    </div>
  );
}

function renderControl(
  exercise: Exercise,
  value: AnswerValue,
  onChange: (value: AnswerValue) => void,
  revealed: boolean,
  check?: AnswerCheck | null,
) {
  switch (exercise.kind) {
    case "word-match":
      return (
        <MatchAnswer
          lefts={exercise.lefts}
          rights={exercise.rights}
          correctPairs={(check?.correctAnswer as MatchPair[] | null) ?? exercise.pairs}
          value={value}
          onChange={onChange}
          revealed={revealed}
        />
      );

    case "multiple-choice":
      return (
        <div className="space-y-3">
          <p className="font-display text-[17px] leading-relaxed text-ink">{exercise.stem}</p>
          <ChoiceAnswer
            options={exercise.options}
            correctLabels={correctLabelsOf(check)}
            value={value}
            onChange={onChange}
            revealed={revealed}
          />
        </div>
      );

    case "cloze":
      return (
        <div className="space-y-3">
          <p className="font-display text-[17px] leading-relaxed text-ink">
            {exercise.sentence.split("{{blank}}").map((part, index, arr) => (
              <span key={`${part}-${index}`}>
                {part}
                {index < arr.length - 1 && (
                  <span className="mx-1 inline-block min-w-20 border-b border-line-strong text-center align-baseline text-brand">
                    {revealed ? expectedText(check, exercise.answer) : "\u00A0"}
                  </span>
                )}
              </span>
            ))}
          </p>
          <ChoiceAnswer
            options={exercise.options}
            correctLabels={correctLabelsOf(check)}
            value={value}
            onChange={onChange}
            revealed={revealed}
          />
        </div>
      );

    case "spelling":
      return (
        <div className="space-y-3">
          <div className="rounded-sm bg-sunken/60 px-3 py-2">
            <span className="text-2xs text-ink-muted">释义</span>
            <p className="text-[13.5px] text-ink">{exercise.definition}</p>
          </div>
          <TextAnswer
            value={value}
            onChange={onChange}
            revealed={revealed}
            expected={expectedText(check, exercise.answer)}
            hint={exercise.hint}
            placeholder="输入完整拼写"
          />
        </div>
      );

    case "dictation":
      return (
        <DictationAnswer
          audioText={exercise.audioText}
          expected={expectedText(check, exercise.answer)}
          hint={exercise.hint}
          value={value}
          onChange={onChange}
          revealed={revealed}
        />
      );

    case "translate":
      return (
        <div className="space-y-3">
          <div className="rounded-sm border-l-2 border-brand/50 bg-brand-tint/25 px-3 py-2">
            <span className="text-2xs text-ink-muted">
              {exercise.sourceLang === "zh" ? "中文" : "英文"}
            </span>
            <p className="text-[14px] leading-relaxed text-ink">{exercise.source}</p>
          </div>
          <TextAnswer
            value={value}
            onChange={onChange}
            revealed={revealed}
            expected={expectedText(check, exercise.answer)}
            multiline
            placeholder="输入译文"
          />
        </div>
      );

    case "sentence-order":
      return (
        <OrderAnswer
          tokens={exercise.tokens}
          value={value}
          onChange={onChange}
          revealed={revealed}
        />
      );

    case "error-correction":
      return (
        <div className="space-y-3">
          <div className="rounded-sm border-l-2 border-danger/50 bg-danger/[0.04] px-3 py-2">
            <span className="text-2xs text-ink-muted">原句 · 错误类型：{exercise.errorTag}</span>
            <p className="text-[14px] leading-relaxed text-ink">{exercise.wrong}</p>
          </div>
          <TextAnswer
            value={value}
            onChange={onChange}
            revealed={revealed}
            expected={expectedText(check, exercise.answer)}
            multiline
            placeholder="写出改正后的句子"
          />
        </div>
      );

    case "rewrite":
      return (
        <div className="space-y-3">
          <div className="rounded-sm bg-sunken/60 px-3 py-2">
            <span className="text-2xs text-ink-muted">原句</span>
            <p className="text-[13.5px] leading-relaxed text-ink">{exercise.source}</p>
          </div>
          <p className="text-2xs text-ink-muted">改写要求：{exercise.instruction}</p>
          <TextAnswer
            value={value}
            onChange={onChange}
            revealed={revealed}
            expected={expectedText(check, exercise.answer)}
            multiline
            placeholder="写出改写后的句子"
          />
        </div>
      );

    case "shadowing":
      return (
        <ShadowingAnswer
          text={exercise.text}
          chunks={exercise.chunks}
          translation={exercise.translation}
          expected={expectedText(check, exercise.text)}
          value={value}
          onChange={onChange}
          revealed={revealed}
        />
      );

    case "roleplay":
      return (
        <RoleplayAnswer
          scenario={exercise.scenario}
          turns={exercise.turns}
          expectedById={expectedByIdOf(check)}
          value={value}
          onChange={onChange}
          revealed={revealed}
        />
      );

    default:
      return null;
  }
}
