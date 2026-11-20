import { useMemo, useState } from "react";
import { useSearchParams } from "react-router";
import { LayoutGrid, Network, Search, X } from "lucide-react";
import type {
  ApiResult,
  DifficultyFilter,
  KindFilter,
  KnowledgeDetail,
  KnowledgeFilter,
  MasteryFilter,
} from "@/types";
import { DEFAULT_KNOWLEDGE_FILTER, KNOWLEDGE_KIND_LABEL } from "@/types/knowledge";
import { DIFFICULTY_LABEL, MASTERY_STATE_LABEL } from "@/types/vocabulary";
import { knowledgeService } from "@/services/api";
import { useQuery } from "@/hooks/use-query";
import { cn } from "@/lib/utils";
import { PageContainer } from "@/components/common/page-container";
import { PageHeader } from "@/components/common/page-header";
import { ErrorState, LoadingState } from "@/components/common/states";
import { KnowledgeCard } from "@/components/common/knowledge-card";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { KnowledgeGraph } from "./components/knowledge-graph";
import { KnowledgeDetailPanel } from "./components/knowledge-detail-panel";

type ViewMode = "graph" | "list";

export function KnowledgePage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const [view, setView] = useState<ViewMode>("graph");
  const [filter, setFilter] = useState<KnowledgeFilter>(DEFAULT_KNOWLEDGE_FILTER);
  const selectedId = searchParams.get("id");

  const graphQuery = useQuery((options) => knowledgeService.getKnowledgeGraph(options), []);
  const listQuery = useQuery(
    (options) => knowledgeService.getKnowledgeList(filter, options),
    [filter.keyword, filter.kind, filter.difficulty, filter.mastery],
  );
  const detailQuery = useQuery<KnowledgeDetail | null>(
    (options) =>
      selectedId
        ? knowledgeService.getKnowledgeDetail(selectedId, options)
        : Promise.resolve<ApiResult<KnowledgeDetail | null>>({ ok: true, data: null }),
    [selectedId],
  );

  const matchedIds = useMemo(
    () => new Set((listQuery.data ?? []).map((item) => item.id)),
    [listQuery.data],
  );

  const setSelected = (id: string | null) => {
    if (id) setSearchParams({ id }, { replace: true });
    else setSearchParams({}, { replace: true });
  };

  const updateFilter = <K extends keyof KnowledgeFilter>(key: K, value: KnowledgeFilter[K]) =>
    setFilter((prev) => ({ ...prev, [key]: value }));

  const detail = selectedId ? (detailQuery.data ?? null) : null;

  return (
    <PageContainer>
      <PageHeader
        title="知识库"
        description="单词、词组、语法与句子共用同一套 ID 与关联模型。图谱不是装饰：每个节点都对应真实条目，连线来自实际引用关系。"
        meta={
          <>
            <span className="flex items-center gap-1 text-2xs text-ink-muted">
              <Network className="size-3" />
              {graphQuery.data
                ? `${graphQuery.data.nodes.length} 个知识对象 · ${graphQuery.data.edges.length} 条关联`
                : "正在读取知识图谱"}
            </span>
            <span className="text-2xs text-ink-muted">
              当前筛选出 {listQuery.data?.length ?? 0} 项
            </span>
          </>
        }
        actions={
          <div className="flex rounded-md bg-sunken p-0.5" role="group" aria-label="视图切换">
            <button
              type="button"
              onClick={() => setView("graph")}
              aria-pressed={view === "graph"}
              className={cn(
                "flex items-center gap-1.5 rounded-sm px-2.5 py-1 text-2xs transition-colors",
                view === "graph" ? "bg-surface text-ink shadow-hairline" : "text-ink-muted",
              )}
            >
              <Network className="size-3" />
              图谱
            </button>
            <button
              type="button"
              onClick={() => setView("list")}
              aria-pressed={view === "list"}
              className={cn(
                "flex items-center gap-1.5 rounded-sm px-2.5 py-1 text-2xs transition-colors",
                view === "list" ? "bg-surface text-ink shadow-hairline" : "text-ink-muted",
              )}
            >
              <LayoutGrid className="size-3" />
              列表
            </button>
          </div>
        }
      />

      {/* 三个独立筛选轴 + 关键词搜索 */}
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <div className="relative min-w-48 flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-ink-muted" />
          <Input
            value={filter.keyword}
            onChange={(event) => updateFilter("keyword", event.target.value)}
            placeholder="搜索单词、释义或标签…"
            className="h-8 pl-8 text-[13px]"
            aria-label="搜索知识库"
          />
          {filter.keyword && (
            <button
              type="button"
              onClick={() => updateFilter("keyword", "")}
              className="absolute top-1/2 right-2 -translate-y-1/2 text-ink-muted hover:text-ink"
              aria-label="清空搜索"
            >
              <X className="size-3.5" />
            </button>
          )}
        </div>

        <Select value={filter.kind} onValueChange={(value) => updateFilter("kind", value as KindFilter)}>
          <SelectTrigger aria-label="按类型筛选">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">全部类型</SelectItem>
            {(Object.keys(KNOWLEDGE_KIND_LABEL) as (keyof typeof KNOWLEDGE_KIND_LABEL)[]).map(
              (kind) => (
                <SelectItem key={kind} value={kind}>
                  {KNOWLEDGE_KIND_LABEL[kind]}
                </SelectItem>
              ),
            )}
          </SelectContent>
        </Select>

        <Select
          value={String(filter.difficulty)}
          onValueChange={(value) =>
            updateFilter("difficulty", (value === "all" ? "all" : Number(value)) as DifficultyFilter)
          }
        >
          <SelectTrigger aria-label="按难度筛选">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">全部难度</SelectItem>
            {([1, 2, 3, 4, 5] as const).map((level) => (
              <SelectItem key={level} value={String(level)}>
                {DIFFICULTY_LABEL[level]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={filter.mastery}
          onValueChange={(value) => updateFilter("mastery", value as MasteryFilter)}
        >
          <SelectTrigger aria-label="按掌握状态筛选">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">全部状态</SelectItem>
            {(Object.keys(MASTERY_STATE_LABEL) as (keyof typeof MASTERY_STATE_LABEL)[]).map(
              (state) => (
                <SelectItem key={state} value={state}>
                  {MASTERY_STATE_LABEL[state]}
                </SelectItem>
              ),
            )}
          </SelectContent>
        </Select>

        {(filter.keyword ||
          filter.kind !== "all" ||
          filter.difficulty !== "all" ||
          filter.mastery !== "all") && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setFilter(DEFAULT_KNOWLEDGE_FILTER)}
            className="text-ink-muted"
          >
            重置
          </Button>
        )}
      </div>

      {graphQuery.error && <ErrorState error={graphQuery.error} onRetry={graphQuery.reload} />}
      {graphQuery.initial && <LoadingState variant="chart" />}

      {graphQuery.data && (
        <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_21rem]">
          <div className="min-w-0">
            {view === "graph" ? (
              <KnowledgeGraph
                nodes={graphQuery.data.nodes}
                edges={graphQuery.data.edges}
                selectedId={selectedId}
                matchedIds={matchedIds}
                onSelect={setSelected}
                className="h-[34rem]"
              />
            ) : (
              <div className="space-y-2">
                {listQuery.initial && <LoadingState rows={5} />}
                <div className="grid gap-2 sm:grid-cols-2">
                  {(listQuery.data ?? []).map((item) => (
                    <KnowledgeCard
                      key={item.id}
                      item={item}
                      active={selectedId === item.id}
                      onClick={(next) => setSelected(next.id)}
                    />
                  ))}
                </div>
                {listQuery.data?.length === 0 && (
                  <div className="rounded-md border border-dashed border-line px-4 py-10 text-center text-2xs text-ink-muted">
                    没有符合当前筛选条件的条目。换个关键词或清空筛选试试。
                  </div>
                )}
              </div>
            )}
          </div>

          <aside className="min-w-0">
            <KnowledgeDetailPanel
              detail={detail}
              loading={selectedId !== null && detailQuery.loading}
              onClose={() => setSelected(null)}
              onOpenRelated={(id) => setSelected(id)}
            />
          </aside>
        </div>
      )}
    </PageContainer>
  );
}
