import { Link2, Network, Volume2, X } from "lucide-react";
import type { KnowledgeDetail } from "@/types";
import { DIFFICULTY_LABEL, MASTERY_STATE_LABEL, PART_OF_SPEECH_LABEL } from "@/types/vocabulary";
import { GRAMMAR_CATEGORY_LABEL } from "@/types/grammar";
import { SENTENCE_CATEGORY_LABEL } from "@/types/sentence";
import { formatRelativeDay, formatRelativeTime } from "@/lib/date";
import { speak } from "@/lib/speech";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { MasteryBar } from "@/components/common/mastery-bar";
import { LoadingState } from "@/components/common/states";

interface KnowledgeDetailPanelProps {
  detail: KnowledgeDetail | null;
  loading: boolean;
  onClose: () => void;
  onOpenRelated: (id: string) => void;
}

export function KnowledgeDetailPanel({
  detail,
  loading,
  onClose,
  onOpenRelated,
}: KnowledgeDetailPanelProps) {
  if (loading && !detail) {
    return (
      <div className="rounded-md border border-line bg-surface p-4">
        <LoadingState variant="text" />
      </div>
    );
  }

  if (!detail) {
    return (
      <div className="rounded-md border border-dashed border-line px-4 py-10 text-center">
        <Network className="mx-auto size-5 text-ink-muted/70" />
        <p className="mt-2 text-[13px] text-ink-soft">选择一个节点查看详情</p>
        <p className="mt-1 text-2xs text-ink-muted">
          可以拖动节点重新排布，滚轮缩放，点击空白处取消选中。
        </p>
      </div>
    );
  }

  const title =
    detail.kind === "word" || detail.kind === "phrase"
      ? detail.word.term
      : detail.kind === "grammar"
        ? detail.grammar.title
        : detail.sentence.en;

  const subtitle =
    detail.kind === "word" || detail.kind === "phrase"
      ? detail.word.phonetic
      : detail.kind === "grammar"
        ? detail.grammar.titleEn
        : detail.sentence.zh;

  return (
    <div className="flex max-h-[calc(100vh-12rem)] flex-col overflow-hidden rounded-md border border-line bg-surface">
      <header className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
        <div className="min-w-0">
          <h3 className="font-display text-[19px] leading-tight break-words text-ink">{title}</h3>
          <p className="mt-0.5 truncate text-2xs text-ink-muted">{subtitle}</p>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {(detail.kind === "word" || detail.kind === "phrase") && (
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => speak(detail.word.term)}
              aria-label="朗读单词"
            >
              <Volume2 />
            </Button>
          )}
          <Button variant="ghost" size="icon-sm" onClick={onClose} aria-label="关闭详情">
            <X />
          </Button>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        {(detail.kind === "word" || detail.kind === "phrase") && (
          <WordDetail detail={detail} onOpenRelated={onOpenRelated} />
        )}
        {detail.kind === "grammar" && (
          <GrammarDetailView detail={detail} onOpenRelated={onOpenRelated} />
        )}
        {detail.kind === "sentence" && (
          <SentenceDetailView detail={detail} onOpenRelated={onOpenRelated} />
        )}
      </div>
    </div>
  );
}

function MetaRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-3 py-0.5">
      <span className="shrink-0 text-2xs text-ink-muted">{label}</span>
      <span className="text-right text-2xs text-ink-soft">{value}</span>
    </div>
  );
}

function RelatedChips({
  items,
  onOpen,
  emptyLabel,
}: {
  items: { id: string; primary: string }[];
  onOpen: (id: string) => void;
  emptyLabel: string;
}) {
  if (items.length === 0) {
    return <p className="text-2xs text-ink-muted">{emptyLabel}</p>;
  }
  return (
    <div className="flex flex-wrap gap-1.5">
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          onClick={() => onOpen(item.id)}
          className="inline-flex items-center gap-1 rounded-sm bg-sunken px-1.5 py-0.5 text-2xs text-ink-soft transition-colors hover:bg-brand-tint hover:text-brand"
        >
          <Link2 className="size-2.5" />
          {item.primary}
        </button>
      ))}
    </div>
  );
}

function LearningHistory({
  mastery,
  state,
  reviewCount,
  lastReviewedAt,
  dueAt,
}: {
  mastery: number;
  state: keyof typeof MASTERY_STATE_LABEL;
  reviewCount: number;
  lastReviewedAt: string | null;
  dueAt: string | null;
}) {
  return (
    <section className="mt-4">
      <h4 className="text-2xs font-medium text-ink-muted">学习历史</h4>
      <div className="mt-1.5 space-y-1">
        <MasteryBar value={mastery} label="掌握度" />
        <MetaRow label="状态" value={MASTERY_STATE_LABEL[state]} />
        <MetaRow label="复习次数" value={`${reviewCount} 次`} />
        <MetaRow
          label="上次复习"
          value={lastReviewedAt ? formatRelativeTime(lastReviewedAt) : "尚未复习"}
        />
        <MetaRow label="下次到期" value={dueAt ? formatRelativeDay(dueAt) : "—"} />
      </div>
    </section>
  );
}

function WordDetail({
  detail,
  onOpenRelated,
}: {
  detail: Extract<KnowledgeDetail, { kind: "word" | "phrase" }>;
  onOpenRelated: (id: string) => void;
}) {
  const word = detail.word;

  return (
    <div>
      <div className="flex flex-wrap items-center gap-1.5">
        <Badge variant="brand">{PART_OF_SPEECH_LABEL[word.pos]}</Badge>
        <Badge variant="outline">{DIFFICULTY_LABEL[word.difficulty]}</Badge>
        <Badge variant="default">{word.level}</Badge>
        {word.contexts.map((context) => (
          <Badge key={context} variant="default">
            {context}
          </Badge>
        ))}
      </div>

      <section className="mt-3">
        <h4 className="text-2xs font-medium text-ink-muted">释义</h4>
        <ul className="mt-1 space-y-1">
          {word.meanings.map((meaning) => (
            <li key={meaning.definition} className="text-[13px] leading-relaxed text-ink">
              {meaning.definition}
              {meaning.definitionEn && (
                <span className="mt-0.5 block text-2xs text-ink-muted">
                  {meaning.definitionEn}
                </span>
              )}
            </li>
          ))}
        </ul>
      </section>

      <Separator className="my-3" />

      <section>
        <h4 className="text-2xs font-medium text-ink-muted">例句</h4>
        <ul className="mt-1.5 space-y-2">
          {word.examples.map((example) => (
            <li key={example.id} className="border-l-2 border-line-strong pl-2.5">
              <p className="text-[12.5px] leading-relaxed text-ink-soft">{example.en}</p>
              <p className="text-2xs text-ink-muted">{example.zh}</p>
            </li>
          ))}
        </ul>
      </section>

      <Separator className="my-3" />

      <section>
        <h4 className="text-2xs font-medium text-ink-muted">相关词汇</h4>
        <div className="mt-1.5">
          <RelatedChips
            items={detail.relatedWords.map((r) => ({ id: r.id, primary: r.primary }))}
            onOpen={onOpenRelated}
            emptyLabel="暂无相关词汇"
          />
        </div>
      </section>

      <section className="mt-3">
        <h4 className="text-2xs font-medium text-ink-muted">相关语法</h4>
        <div className="mt-1.5">
          <RelatedChips
            items={detail.grammarPoints.map((g) => ({ id: g.id, primary: g.title }))}
            onOpen={onOpenRelated}
            emptyLabel="尚未关联语法点"
          />
        </div>
      </section>

      {detail.sentences.length > 0 && (
        <section className="mt-3">
          <h4 className="text-2xs font-medium text-ink-muted">出现在这些句子中</h4>
          <ul className="mt-1.5 space-y-1.5">
            {detail.sentences.map((sentence) => (
              <li key={sentence.id}>
                <button
                  type="button"
                  onClick={() => onOpenRelated(sentence.id)}
                  className="w-full text-left text-2xs leading-relaxed text-ink-soft transition-colors hover:text-brand"
                >
                  {sentence.en}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      <LearningHistory
        mastery={detail.state?.mastery ?? 0}
        state={detail.state?.state ?? "new"}
        reviewCount={detail.state?.reviewCount ?? 0}
        lastReviewedAt={detail.state?.lastReviewedAt ?? null}
        dueAt={detail.state?.dueAt ?? null}
      />
    </div>
  );
}

function GrammarDetailView({
  detail,
  onOpenRelated,
}: {
  detail: Extract<KnowledgeDetail, { kind: "grammar" }>;
  onOpenRelated: (id: string) => void;
}) {
  const point = detail.grammar;

  return (
    <div>
      <div className="flex flex-wrap items-center gap-1.5">
        <Badge variant="success">{GRAMMAR_CATEGORY_LABEL[point.category]}</Badge>
        <Badge variant="outline">{DIFFICULTY_LABEL[point.difficulty]}</Badge>
        <Badge variant="default">{point.level}</Badge>
      </div>

      <p className="mt-2.5 text-[12.5px] leading-relaxed text-ink-soft">{point.summary}</p>

      <div className="mt-2.5 rounded-sm bg-sunken/70 px-2.5 py-2">
        <span className="text-2xs text-ink-muted">结构</span>
        <code className="mt-0.5 block font-mono text-[12px] text-ink">{point.structure}</code>
      </div>

      <p className="mt-2.5 text-2xs leading-relaxed text-ink-muted">{point.explanation}</p>

      <Separator className="my-3" />

      <section>
        <h4 className="text-2xs font-medium text-ink-muted">例句</h4>
        <ul className="mt-1.5 space-y-2">
          {point.examples.map((example) => (
            <li key={example.id} className="border-l-2 border-line-strong pl-2.5">
              <p className="text-[12.5px] leading-relaxed text-ink-soft">{example.en}</p>
              <p className="text-2xs text-ink-muted">{example.zh}</p>
              {example.note && <p className="text-2xs text-ink-muted/80">— {example.note}</p>}
            </li>
          ))}
        </ul>
      </section>

      <Separator className="my-3" />

      <section>
        <h4 className="text-2xs font-medium text-ink-muted">常见错误</h4>
        <ul className="mt-1.5 space-y-2">
          {point.mistakes.map((mistake) => (
            <li key={mistake.id} className="rounded-sm bg-danger/[0.04] px-2 py-1.5">
              <p className="text-2xs text-danger line-through decoration-danger/40">
                {mistake.wrong}
              </p>
              <p className="text-2xs text-success">{mistake.correct}</p>
              <p className="mt-0.5 text-2xs text-ink-muted">{mistake.reason}</p>
            </li>
          ))}
        </ul>
      </section>

      <section className="mt-3">
        <h4 className="text-2xs font-medium text-ink-muted">相关词汇</h4>
        <div className="mt-1.5">
          <RelatedChips
            items={detail.words.map((w) => ({ id: w.id, primary: w.primary }))}
            onOpen={onOpenRelated}
            emptyLabel="尚未关联词汇"
          />
        </div>
      </section>

      {detail.sentences.length > 0 && (
        <section className="mt-3">
          <h4 className="text-2xs font-medium text-ink-muted">应用句子</h4>
          <ul className="mt-1.5 space-y-1.5">
            {detail.sentences.map((sentence) => (
              <li key={sentence.id}>
                <button
                  type="button"
                  onClick={() => onOpenRelated(sentence.id)}
                  className="text-left text-2xs leading-relaxed text-ink-soft hover:text-brand"
                >
                  {sentence.en}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      <LearningHistory
        mastery={detail.state?.mastery ?? 0}
        state={detail.state?.state ?? "new"}
        reviewCount={detail.state?.reviewCount ?? 0}
        lastReviewedAt={detail.state?.lastReviewedAt ?? null}
        dueAt={detail.state?.dueAt ?? null}
      />
    </div>
  );
}

function SentenceDetailView({
  detail,
  onOpenRelated,
}: {
  detail: Extract<KnowledgeDetail, { kind: "sentence" }>;
  onOpenRelated: (id: string) => void;
}) {
  const sentence = detail.sentence;

  return (
    <div>
      <div className="flex flex-wrap items-center gap-1.5">
        <Badge variant="default">{SENTENCE_CATEGORY_LABEL[sentence.category]}</Badge>
        <Badge variant="outline">{DIFFICULTY_LABEL[sentence.difficulty]}</Badge>
        <Badge variant="brand">{sentence.level}</Badge>
      </div>

      <div className="mt-2.5 rounded-sm bg-raised px-2.5 py-2">
        <div className="flex items-start justify-between gap-2">
          <p className="font-display text-[15px] leading-relaxed text-ink">{sentence.en}</p>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => speak(sentence.en)}
            aria-label="朗读句子"
          >
            <Volume2 />
          </Button>
        </div>
        <div className="mt-1.5 flex flex-wrap gap-1">
          {sentence.chunks.map((chunk) => (
            <span key={chunk} className="rounded-sm bg-brand-tint/60 px-1.5 py-0.5 text-2xs text-brand">
              {chunk}
            </span>
          ))}
        </div>
        <p className="mt-1.5 text-2xs text-ink-muted">{sentence.zh}</p>
      </div>

      <section className="mt-3">
        <h4 className="text-2xs font-medium text-ink-muted">相关语法</h4>
        <div className="mt-1.5">
          <RelatedChips
            items={detail.grammarPoints.map((g) => ({ id: g.id, primary: g.title }))}
            onOpen={onOpenRelated}
            emptyLabel="尚未关联语法点"
          />
        </div>
      </section>

      <section className="mt-3">
        <h4 className="text-2xs font-medium text-ink-muted">关键词汇</h4>
        <div className="mt-1.5">
          <RelatedChips
            items={detail.words.map((w) => ({ id: w.id, primary: w.primary }))}
            onOpen={onOpenRelated}
            emptyLabel="尚未关联词汇"
          />
        </div>
      </section>

      <LearningHistory
        mastery={detail.state?.mastery ?? 0}
        state={detail.state?.state ?? "new"}
        reviewCount={detail.state?.reviewCount ?? 0}
        lastReviewedAt={detail.state?.lastReviewedAt ?? null}
        dueAt={detail.state?.dueAt ?? null}
      />
    </div>
  );
}
