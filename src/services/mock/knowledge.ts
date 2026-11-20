import type {
  Difficulty,
  KnowledgeEdge,
  KnowledgeGraph,
  KnowledgeKind,
  KnowledgeNode,
  KnowledgeSummary,
  MasteryState,
  RelationKind,
} from "@/types";
import { allGrammar } from "./grammar";
import { allSentences } from "./sentences";
import { hintMastery, hintState } from "./state-hints";
import { allWords } from "./vocabulary";

const now = new Date();

/** 以三个语义簇为起点做确定性布局，避免图谱每次打开都"跳一次" */
const CLUSTER_ANCHOR: Record<KnowledgeKind, { x: number; y: number }> = {
  word: { x: 0.34, y: 0.42 },
  phrase: { x: 0.5, y: 0.2 },
  grammar: { x: 0.7, y: 0.52 },
  sentence: { x: 0.42, y: 0.76 },
};

function seedPosition(kind: KnowledgeKind, index: number, total: number) {
  const anchor = CLUSTER_ANCHOR[kind];
  const golden = 2.399963;
  const radius = 0.1 + 0.055 * Math.sqrt(index + 1);
  const angle = index * golden;
  const jitter = total > 1 ? 1 : 0;
  return {
    x: Math.min(0.95, Math.max(0.05, anchor.x + Math.cos(angle) * radius * jitter)),
    y: Math.min(0.92, Math.max(0.08, anchor.y + Math.sin(angle) * radius * jitter * 0.9)),
  };
}

function buildNodes(): KnowledgeNode[] {
  const words = allWords();
  const grammar = allGrammar();
  const sentences = allSentences();

  const wordNodes: KnowledgeNode[] = words.map((w, i) => ({
    id: w.id,
    kind: w.pos === "phrase" ? "phrase" : "word",
    label: w.term,
    sublabel: w.meanings[0]?.definition ?? "",
    difficulty: w.difficulty,
    mastery: hintMastery(w.id),
    state: hintState(w.id),
    weight: 1 + hintMastery(w.id) / 100,
    ...seedPosition(
      w.pos === "phrase" ? "phrase" : "word",
      i,
      words.length,
    ),
  }));

  const grammarNodes: KnowledgeNode[] = grammar.map((g, i) => ({
    id: g.id,
    kind: "grammar",
    label: g.title,
    sublabel: g.titleEn,
    difficulty: g.difficulty,
    mastery: hintMastery(g.id),
    state: hintState(g.id),
    weight: 1.4 + hintMastery(g.id) / 120,
    ...seedPosition("grammar", i, grammar.length),
  }));

  const sentenceNodes: KnowledgeNode[] = sentences.map((s, i) => ({
    id: s.id,
    kind: "sentence",
    label: s.en.length > 28 ? `${s.en.slice(0, 26)}…` : s.en,
    sublabel: s.zh,
    difficulty: s.difficulty,
    mastery: hintMastery(s.id),
    state: hintState(s.id),
    weight: 1.2 + hintMastery(s.id) / 140,
    ...seedPosition("sentence", i, sentences.length),
  }));

  return [...wordNodes, ...grammarNodes, ...sentenceNodes];
}

function buildEdges(nodes: KnowledgeNode[]): KnowledgeEdge[] {
  const ids = new Set(nodes.map((n) => n.id));
  const edges: KnowledgeEdge[] = [];
  const seen = new Set<string>();

  const push = (source: string, target: string, relation: RelationKind, weight = 1) => {
    if (!ids.has(source) || !ids.has(target) || source === target) return;
    const key = [source, target].sort().join("|");
    if (seen.has(key)) return;
    seen.add(key);
    edges.push({
      id: `e_${source}_${target}`,
      source,
      target,
      relation,
      weight,
    });
  };

  for (const word of allWords()) {
    for (const gid of word.grammarIds) push(word.id, gid, "explains", 0.8);
    for (const rid of word.relatedWordIds) push(word.id, rid, "related", 1);
  }

  for (const g of allGrammar()) {
    for (const wid of g.relatedWordIds) push(g.id, wid, "used-in", 0.7);
  }

  for (const s of allSentences()) {
    for (const gid of s.grammarIds) push(s.id, gid, "used-in", 1.1);
    for (const wid of s.wordIds) push(s.id, wid, "used-in", 1);
  }

  return edges;
}

const graphNodes = buildNodes();

export const knowledgeGraph: KnowledgeGraph = {
  nodes: graphNodes,
  edges: buildEdges(graphNodes),
  updatedAt: now.toISOString(),
};

/** 节点 ID 与原始实体对齐，详情面板无需二次查表 */
export function nodeDegreeMap() {
  const map = new Map<string, number>();
  for (const edge of knowledgeGraph.edges) {
    map.set(edge.source, (map.get(edge.source) ?? 0) + 1);
    map.set(edge.target, (map.get(edge.target) ?? 0) + 1);
  }
  return map;
}

export function neighborsOf(id: string) {
  const ids = new Set<string>();
  for (const edge of knowledgeGraph.edges) {
    if (edge.source === id) ids.add(edge.target);
    if (edge.target === id) ids.add(edge.source);
  }
  return ids;
}

export function toSummary(node: KnowledgeNode, tags: string[]): KnowledgeSummary {
  return {
    id: node.id,
    kind: node.kind,
    primary: node.label,
    secondary: node.sublabel,
    difficulty: node.difficulty as Difficulty,
    mastery: node.mastery,
    state: node.state as MasteryState,
    tags,
    dueAt: null,
  };
}
