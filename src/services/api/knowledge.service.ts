import type {
  ApiResult,
  EntityState,
  GrammarPoint,
  KnowledgeDetail,
  KnowledgeFilter,
  KnowledgeGraph,
  KnowledgeSummary,
  RequestOptions,
  Sentence,
} from "@/types";
import { DEFAULT_KNOWLEDGE_FILTER } from "@/types/knowledge";
import { latency } from "@/services/mock/base";
import { allGrammar, findGrammar } from "@/services/mock/grammar";
import { knowledgeGraph, neighborsOf } from "@/services/mock/knowledge";
import { allSentences, findSentence } from "@/services/mock/sentences";
import { allWords, findWord } from "@/services/mock/vocabulary";
import { getDb } from "@/stores/learning-store";
import { COMMANDS } from "./contracts";
import { serve } from "./transport";

/**
 * 知识库是"内容 + 学习态"的合成视图：
 * 正文来自只读词库，掌握度/到期时间来自统一学习状态。
 * 页面拿到的 DTO 形状与第一阶段一致。
 */

type States = Record<string, EntityState>;

/**
 * 学习态的唯一来源。
 *
 * 内容实体（Vocabulary / GrammarPoint / Sentence）**不再**携带
 * mastery / state / reviewCount / dueAt / strength / lastReviewedAt。
 * 那些字段只存在于 `db.entityStates`，避免"内容里的旧值被当成当前掌握度"。
 */
function stateOf(id: string, states?: States): EntityState | null {
  return (states ?? getDb().entityStates)[id] ?? null;
}

/** 摘要所需的三个学习态字段，取自唯一的 EntityState */
function stateFields(
  id: string,
  states?: States,
): Pick<KnowledgeSummary, "mastery" | "state" | "dueAt"> {
  const state = stateOf(id, states);
  return {
    mastery: state?.mastery ?? 0,
    state: state?.state ?? "new",
    dueAt: state?.dueAt ?? null,
  };
}

function wordSummary(id: string, states?: States): KnowledgeSummary | null {
  const word = findWord(id);
  if (!word) return null;
  return {
    id: word.id,
    kind: word.pos === "phrase" ? "phrase" : "word",
    primary: word.term,
    secondary: word.meanings[0]?.definition ?? "",
    difficulty: word.difficulty,
    ...stateFields(word.id, states),
    tags: word.contexts,
  };
}

function grammarSummary(id: string, states?: States): KnowledgeSummary | null {
  const grammar = findGrammar(id);
  if (!grammar) return null;
  return {
    id: grammar.id,
    kind: "grammar",
    primary: grammar.title,
    secondary: grammar.titleEn,
    difficulty: grammar.difficulty,
    ...stateFields(grammar.id, states),
    tags: [grammar.category],
  };
}

function sentenceSummary(sentence: Sentence, states?: States): KnowledgeSummary {
  return {
    id: sentence.id,
    kind: "sentence",
    primary: sentence.en,
    secondary: sentence.zh,
    difficulty: sentence.difficulty,
    ...stateFields(sentence.id, states),
    tags: [sentence.category],
  };
}

export function listKnowledge(filter: KnowledgeFilter = DEFAULT_KNOWLEDGE_FILTER) {
  const states = getDb().entityStates;
  const keyword = filter.keyword.trim().toLowerCase();

  const summaries: KnowledgeSummary[] = [
    ...allWords().map((word) => {
      return {
        id: word.id,
        kind: (word.pos === "phrase" ? "phrase" : "word") as KnowledgeSummary["kind"],
        primary: word.term,
        secondary: word.meanings[0]?.definition ?? "",
        difficulty: word.difficulty,
        ...stateFields(word.id, states),
        tags: word.contexts,
      };
    }),
    ...allGrammar().map((grammar) => {
      return {
        id: grammar.id,
        kind: "grammar" as const,
        primary: grammar.title,
        secondary: grammar.titleEn,
        difficulty: grammar.difficulty,
        ...stateFields(grammar.id, states),
        tags: [grammar.category],
      };
    }),
    ...allSentences().map((sentence) => sentenceSummary(sentence, states)),
  ];

  return summaries.filter((item) => {
    if (filter.kind !== "all" && item.kind !== filter.kind) return false;
    if (filter.difficulty !== "all" && item.difficulty !== filter.difficulty) return false;
    if (filter.mastery !== "all" && item.state !== filter.mastery) return false;
    if (!keyword) return true;
    return (
      item.primary.toLowerCase().includes(keyword) ||
      item.secondary.toLowerCase().includes(keyword) ||
      item.tags.some((tag) => tag.toLowerCase().includes(keyword))
    );
  });
}

export function getKnowledgeGraph(options: RequestOptions = {}): Promise<ApiResult<KnowledgeGraph>> {
  return serve(
    COMMANDS.knowledgeGraph,
    async () => {
      await latency(options.signal);
      const states = getDb().entityStates;
      return {
        ...knowledgeGraph,
        nodes: knowledgeGraph.nodes.map((node) => {
          const state = states[node.id];
          return state ? { ...node, mastery: state.mastery, state: state.state } : node;
        }),
      };
    },
    { request: options },
  );
}

export function getKnowledgeList(
  filter: KnowledgeFilter = DEFAULT_KNOWLEDGE_FILTER,
  options: RequestOptions = {},
): Promise<ApiResult<KnowledgeSummary[]>> {
  return serve(
    COMMANDS.knowledgeList,
    async () => {
      await latency(options.signal);
      return listKnowledge(filter);
    },
    { args: { filter }, request: options },
  );
}

export function getKnowledgeDetail(
  id: string,
  options: RequestOptions = {},
): Promise<ApiResult<KnowledgeDetail>> {
  return serve(
    COMMANDS.knowledgeDetail,
    async () => {
      await latency(options.signal);
      const node = knowledgeGraph.nodes.find((n) => n.id === id);
      if (!node) throw new Error(`not_found:${id}`);
      const states = getDb().entityStates;

      if (node.kind === "grammar") {
        const grammar = findGrammar(id);
        if (!grammar) throw new Error(`not_found:${id}`);
        return {
          kind: "grammar",
          grammar,
          state: stateOf(id, states),
          words: grammar.relatedWordIds
            .map((wid) => wordSummary(wid, states))
            .filter((s): s is KnowledgeSummary => Boolean(s)),
          sentences: allSentences().filter((s) => s.grammarIds.includes(id)),
        } satisfies KnowledgeDetail;
      }

      if (node.kind === "sentence") {
        const sentence = findSentence(id);
        if (!sentence) throw new Error(`not_found:${id}`);
        return {
          kind: "sentence",
          sentence,
          state: stateOf(id, states),
          grammarPoints: sentence.grammarIds
            .map((gid) => findGrammar(gid))
            .filter((g): g is GrammarPoint => Boolean(g)),
          words: sentence.wordIds
            .map((wid) => wordSummary(wid, states))
            .filter((s): s is KnowledgeSummary => Boolean(s)),
        } satisfies KnowledgeDetail;
      }

      const word = findWord(id);
      if (!word) throw new Error(`not_found:${id}`);
      const payload = {
        word,
        state: stateOf(id, states),
        relatedWords: word.relatedWordIds
          .map((wid) => wordSummary(wid, states))
          .filter((s): s is KnowledgeSummary => Boolean(s)),
        grammarPoints: word.grammarIds
          .map((gid) => findGrammar(gid))
          .filter((g): g is GrammarPoint => Boolean(g)),
        sentences: allSentences().filter((s) => s.wordIds.includes(id)),
      };
      return word.pos === "phrase"
        ? ({ kind: "phrase", ...payload } satisfies KnowledgeDetail)
        : ({ kind: "word", ...payload } satisfies KnowledgeDetail);
    },
    { args: { id }, request: options },
  );
}

export function getNeighbors(
  id: string,
  options: RequestOptions = {},
): Promise<ApiResult<KnowledgeSummary[]>> {
  return serve(
    COMMANDS.knowledgeNeighbors,
    async () => {
      await latency(options.signal);
      const ids = neighborsOf(id);
      return Array.from(ids)
        .map((nid) => {
          const node = knowledgeGraph.nodes.find((n) => n.id === nid);
          if (!node) return null;
          if (node.kind === "grammar") return grammarSummary(nid);
          if (node.kind === "sentence") {
            const sentence = findSentence(nid);
            return sentence ? sentenceSummary(sentence, getDb().entityStates) : null;
          }
          return wordSummary(nid);
        })
        .filter((s): s is KnowledgeSummary => Boolean(s));
    },
    { args: { id }, request: options },
  );
}
