import type {
  Difficulty,
  EntityStateSeed,
  MasteryState,
  PartOfSpeech,
  Vocabulary,
} from "@/types";
import { daysAgo, daysAhead } from "./base";

interface WordSeed {
  id: string;
  term: string;
  phonetic: string;
  pos: PartOfSpeech;
  difficulty: Difficulty;
  level: string;
  meanings: { pos: PartOfSpeech; definition: string; definitionEn?: string }[];
  examples: { en: string; zh: string; highlight?: string }[];
  related: string[];
  grammar: string[];
  contexts: string[];
  mastery: number;
  state: MasteryState;
  reviewCount: number;
  dueInDays: number | null;
  strength: number;
  addedDaysAgo: number;
}

const SEEDS: WordSeed[] = [
  {
    id: "w_ubiquitous",
    term: "ubiquitous",
    phonetic: "/juːˈbɪkwɪtəs/",
    pos: "adjective",
    difficulty: 4,
    level: "B2",
    meanings: [
      { pos: "adjective", definition: "无处不在的；普遍存在的", definitionEn: "present or found everywhere" },
    ],
    examples: [
      { en: "Smartphones have become ubiquitous in daily life.", zh: "智能手机在日常生活中已经无处不在。", highlight: "ubiquitous" },
      { en: "The company's logo is ubiquitous on every campus.", zh: "这家公司的标志在每个校园里随处可见。", highlight: "ubiquitous" },
    ],
    related: ["w_pervasive", "w_inevitable"],
    grammar: ["g_present_perfect"],
    contexts: ["academic", "daily"],
    mastery: 72,
    state: "reviewing",
    reviewCount: 6,
    dueInDays: 1,
    strength: 0.62,
    addedDaysAgo: 34,
  },
  {
    id: "w_resilient",
    term: "resilient",
    phonetic: "/rɪˈzɪliənt/",
    pos: "adjective",
    difficulty: 4,
    level: "B2",
    meanings: [
      { pos: "adjective", definition: "有韧性的；能迅速恢复的", definitionEn: "able to recover quickly from difficulties" },
    ],
    examples: [
      { en: "Children are often more resilient than adults expect.", zh: "孩子往往比大人以为的更有韧性。", highlight: "resilient" },
    ],
    related: ["w_fragile", "w_sustain"],
    grammar: ["g_comparative"],
    contexts: ["academic", "workplace"],
    mastery: 58,
    state: "learning",
    reviewCount: 3,
    dueInDays: 0,
    strength: 0.44,
    addedDaysAgo: 12,
  },
  {
    id: "w_mitigate",
    term: "mitigate",
    phonetic: "/ˈmɪtɪɡeɪt/",
    pos: "verb",
    difficulty: 5,
    level: "C1",
    meanings: [
      { pos: "verb", definition: "减轻；缓和（不良影响）", definitionEn: "to make something bad less severe" },
    ],
    examples: [
      { en: "Planting trees can mitigate the effects of urban heat.", zh: "植树可以减轻城市热岛效应。", highlight: "mitigate" },
      { en: "The team took steps to mitigate the risk.", zh: "团队采取措施来降低风险。", highlight: "mitigate" },
    ],
    related: ["w_alleviate", "w_sustain"],
    grammar: ["g_modal_verbs"],
    contexts: ["academic", "workplace"],
    mastery: 24,
    state: "learning",
    reviewCount: 1,
    dueInDays: 0,
    strength: 0.22,
    addedDaysAgo: 5,
  },
  {
    id: "w_ambiguous",
    term: "ambiguous",
    phonetic: "/æmˈbɪɡjuəs/",
    pos: "adjective",
    difficulty: 5,
    level: "C1",
    meanings: [
      { pos: "adjective", definition: "模棱两可的；含糊不清的", definitionEn: "open to more than one interpretation" },
    ],
    examples: [
      { en: "The wording of the contract is deliberately ambiguous.", zh: "合同的措辞被刻意写得含糊。", highlight: "ambiguous" },
    ],
    related: ["w_coherent", "w_scrutiny"],
    grammar: ["g_relative_clause"],
    contexts: ["academic", "workplace"],
    mastery: 16,
    state: "new",
    reviewCount: 0,
    dueInDays: 1,
    strength: 0.12,
    addedDaysAgo: 2,
  },
  {
    id: "w_deliberate",
    term: "deliberate",
    phonetic: "/dɪˈlɪbərət/",
    pos: "adjective",
    difficulty: 4,
    level: "B2",
    meanings: [
      { pos: "adjective", definition: "深思熟虑的；故意的", definitionEn: "done consciously and intentionally" },
      { pos: "verb", definition: "仔细考虑（deliberate on）", definitionEn: "to think carefully about something" },
    ],
    examples: [
      { en: "Her silence was a deliberate choice, not an accident.", zh: "她的沉默是有意为之，并非偶然。", highlight: "deliberate" },
    ],
    related: ["w_contemplate", "w_diligent"],
    grammar: ["g_past_simple"],
    contexts: ["academic", "workplace"],
    mastery: 66,
    state: "reviewing",
    reviewCount: 5,
    dueInDays: 3,
    strength: 0.58,
    addedDaysAgo: 28,
  },
  {
    id: "w_scrutiny",
    term: "scrutiny",
    phonetic: "/ˈskruːtəni/",
    pos: "noun",
    difficulty: 5,
    level: "C1",
    meanings: [
      { pos: "noun", definition: "仔细审查；细看", definitionEn: "critical observation or examination" },
    ],
    examples: [
      { en: "The proposal did not survive public scrutiny.", zh: "该提案没能经受住公众的审查。", highlight: "scrutiny" },
    ],
    related: ["w_ambiguous", "w_perceive"],
    grammar: ["g_passive_voice"],
    contexts: ["academic", "workplace"],
    mastery: 31,
    state: "learning",
    reviewCount: 2,
    dueInDays: 5,
    strength: 0.3,
    addedDaysAgo: 7,
  },
  {
    id: "w_coherent",
    term: "coherent",
    phonetic: "/kəʊˈhɪərənt/",
    pos: "adjective",
    difficulty: 4,
    level: "B2",
    meanings: [
      { pos: "adjective", definition: "连贯的；条理清楚的", definitionEn: "logical and consistent" },
    ],
    examples: [
      { en: "A coherent argument needs evidence at every step.", zh: "一个连贯的论点需要在每一步都有证据。", highlight: "coherent" },
    ],
    related: ["w_ambiguous", "w_profound"],
    grammar: ["g_sentence_structure"],
    contexts: ["academic", "writing"],
    mastery: 84,
    state: "mastered",
    reviewCount: 9,
    dueInDays: 12,
    strength: 0.88,
    addedDaysAgo: 60,
  },
  {
    id: "w_accumulate",
    term: "accumulate",
    phonetic: "/əˈkjuːmjəleɪt/",
    pos: "verb",
    difficulty: 3,
    level: "B1",
    meanings: [
      { pos: "verb", definition: "积累；逐渐增加", definitionEn: "to gather together over time" },
    ],
    examples: [
      { en: "Small habits accumulate into visible progress.", zh: "微小的习惯会积累成可见的进步。", highlight: "accumulate" },
    ],
    related: ["w_sustain", "w_diligent"],
    grammar: ["g_present_perfect"],
    contexts: ["daily", "academic"],
    mastery: 91,
    state: "mastered",
    reviewCount: 11,
    dueInDays: 18,
    strength: 0.92,
    addedDaysAgo: 72,
  },
  {
    id: "w_reluctant",
    term: "reluctant",
    phonetic: "/rɪˈlʌktənt/",
    pos: "adjective",
    difficulty: 3,
    level: "B1",
    meanings: [
      { pos: "adjective", definition: "不情愿的；勉强的", definitionEn: "unwilling and hesitant" },
    ],
    examples: [
      { en: "He was reluctant to admit that he needed help.", zh: "他不愿承认自己需要帮助。", highlight: "reluctant" },
    ],
    related: ["w_deliberate"],
    grammar: ["g_infinitive"],
    contexts: ["daily", "workplace"],
    mastery: 47,
    state: "reviewing",
    reviewCount: 4,
    dueInDays: 0,
    strength: 0.4,
    addedDaysAgo: 19,
  },
  {
    id: "w_contemplate",
    term: "contemplate",
    phonetic: "/ˈkɒntəmpleɪt/",
    pos: "verb",
    difficulty: 4,
    level: "B2",
    meanings: [
      { pos: "verb", definition: "沉思；考虑", definitionEn: "to think about something deeply" },
    ],
    examples: [
      { en: "She sat by the window and contemplated the question.", zh: "她坐在窗边，思考着这个问题。", highlight: "contemplated" },
    ],
    related: ["w_deliberate", "w_profound"],
    grammar: ["g_past_simple"],
    contexts: ["academic", "daily"],
    mastery: 26,
    state: "learning",
    reviewCount: 1,
    dueInDays: 1,
    strength: 0.25,
    addedDaysAgo: 4,
  },
  {
    id: "w_sustain",
    term: "sustain",
    phonetic: "/səˈsteɪn/",
    pos: "verb",
    difficulty: 3,
    level: "B1",
    meanings: [
      { pos: "verb", definition: "维持；支撑", definitionEn: "to keep something going over time" },
    ],
    examples: [
      { en: "It is hard to sustain attention for ninety minutes.", zh: "很难维持九十分钟的注意力。", highlight: "sustain" },
    ],
    related: ["w_accumulate", "w_resilient"],
    grammar: ["g_infinitive"],
    contexts: ["academic", "daily"],
    mastery: 63,
    state: "reviewing",
    reviewCount: 5,
    dueInDays: 4,
    strength: 0.55,
    addedDaysAgo: 22,
  },
  {
    id: "w_inevitable",
    term: "inevitable",
    phonetic: "/ɪnˈevɪtəbl/",
    pos: "adjective",
    difficulty: 3,
    level: "B1",
    meanings: [
      { pos: "adjective", definition: "不可避免的；必然发生的", definitionEn: "certain to happen" },
    ],
    examples: [
      { en: "Some mistakes are inevitable when you learn a language.", zh: "学一门语言时，有些错误是不可避免的。", highlight: "inevitable" },
    ],
    related: ["w_ubiquitous"],
    grammar: ["g_passive_voice"],
    contexts: ["daily", "academic"],
    mastery: 78,
    state: "mastered",
    reviewCount: 7,
    dueInDays: 9,
    strength: 0.79,
    addedDaysAgo: 45,
  },
  {
    id: "w_perceive",
    term: "perceive",
    phonetic: "/pəˈsiːv/",
    pos: "verb",
    difficulty: 3,
    level: "B1",
    meanings: [
      { pos: "verb", definition: "察觉；认为", definitionEn: "to become aware of, or to interpret" },
    ],
    examples: [
      { en: "Learners often perceive grammar as a set of rules to memorize.", zh: "学习者常把语法看作一套需要背诵的规则。", highlight: "perceive" },
    ],
    related: ["w_scrutiny", "w_coherent"],
    grammar: ["g_relative_clause"],
    contexts: ["academic"],
    mastery: 41,
    state: "learning",
    reviewCount: 2,
    dueInDays: 2,
    strength: 0.38,
    addedDaysAgo: 9,
  },
  {
    id: "w_fragile",
    term: "fragile",
    phonetic: "/ˈfrædʒaɪl/",
    pos: "adjective",
    difficulty: 2,
    level: "A2",
    meanings: [
      { pos: "adjective", definition: "易碎的；脆弱的", definitionEn: "easily broken or damaged" },
    ],
    examples: [
      { en: "Motivation is fragile in the first two weeks.", zh: "在最初两周，动力是很脆弱的。", highlight: "fragile" },
    ],
    related: ["w_resilient"],
    grammar: ["g_comparative"],
    contexts: ["daily"],
    mastery: 88,
    state: "mastered",
    reviewCount: 8,
    dueInDays: 15,
    strength: 0.9,
    addedDaysAgo: 51,
  },
  {
    id: "w_diligent",
    term: "diligent",
    phonetic: "/ˈdɪlɪdʒənt/",
    pos: "adjective",
    difficulty: 3,
    level: "B1",
    meanings: [
      { pos: "adjective", definition: "勤勉的；用功的", definitionEn: "showing care and effort in work" },
    ],
    examples: [
      { en: "A diligent review schedule beats a long cramming session.", zh: "勤勉的复习计划胜过长时间的突击。", highlight: "diligent" },
    ],
    related: ["w_accumulate", "w_deliberate"],
    grammar: ["g_comparative"],
    contexts: ["daily", "workplace"],
    mastery: 55,
    state: "reviewing",
    reviewCount: 4,
    dueInDays: 6,
    strength: 0.5,
    addedDaysAgo: 16,
  },
  {
    id: "w_profound",
    term: "profound",
    phonetic: "/prəˈfaʊnd/",
    pos: "adjective",
    difficulty: 4,
    level: "B2",
    meanings: [
      { pos: "adjective", definition: "深刻的；影响深远的", definitionEn: "very great or intense; deep" },
    ],
    examples: [
      { en: "Reading fiction has a profound effect on empathy.", zh: "阅读小说对共情能力有深远影响。", highlight: "profound" },
    ],
    related: ["w_contemplate", "w_coherent"],
    grammar: ["g_article_definite"],
    contexts: ["academic"],
    mastery: 34,
    state: "learning",
    reviewCount: 2,
    dueInDays: 0,
    strength: 0.33,
    addedDaysAgo: 6,
  },
  {
    id: "w_take_into_account",
    term: "take into account",
    phonetic: "/teɪk ˈɪntuː əˈkaʊnt/",
    pos: "phrase",
    difficulty: 3,
    level: "B1",
    meanings: [
      { pos: "phrase", definition: "把……考虑在内", definitionEn: "to consider something when making a decision" },
    ],
    examples: [
      { en: "The plan takes your schedule into account.", zh: "这份计划把你的时间安排考虑在内了。", highlight: "takes your schedule into account" },
    ],
    related: ["w_deliberate"],
    grammar: ["g_prepositional_phrase"],
    contexts: ["workplace", "academic"],
    mastery: 69,
    state: "reviewing",
    reviewCount: 5,
    dueInDays: 3,
    strength: 0.64,
    addedDaysAgo: 26,
  },
  {
    id: "w_in_light_of",
    term: "in light of",
    phonetic: "/ɪn laɪt əv/",
    pos: "phrase",
    difficulty: 4,
    level: "B2",
    meanings: [
      { pos: "phrase", definition: "鉴于；考虑到", definitionEn: "because of, considering" },
    ],
    examples: [
      { en: "In light of recent results, we adjusted the plan.", zh: "鉴于最近的结果，我们调整了计划。", highlight: "In light of" },
    ],
    related: ["w_take_into_account"],
    grammar: ["g_prepositional_phrase"],
    contexts: ["academic", "workplace"],
    mastery: 22,
    state: "new",
    reviewCount: 0,
    dueInDays: 1,
    strength: 0.15,
    addedDaysAgo: 1,
  },
];

/**
 * 初始学习态的**种子输入**（只在首次建库时使用）。
 *
 * 它与内容正文分开导出，是为了让 `Vocabulary` 类型保持"纯内容"：
 * 掌握度/到期时间只允许存在于 `db.entityStates`。
 */
export const wordStateHints: EntityStateSeed[] = SEEDS.map((seed) => ({
  entityId: seed.id,
  kind: seed.pos === "phrase" ? "phrase" : "word",
  mastery: seed.mastery,
  strength: seed.strength,
  state: seed.state,
  reviewCount: seed.reviewCount,
  dueAt: seed.dueInDays === null ? null : daysAhead(seed.dueInDays, 8, 0),
  lastReviewedAt: seed.reviewCount === 0 ? null : daysAgo(Math.min(seed.addedDaysAgo, 3), 21, 30),
  addedAt: daysAgo(seed.addedDaysAgo, 21, 10),
}));

const hintById = new Map(wordStateHints.map((hint) => [hint.entityId, hint]));

export const vocabularySeed: Vocabulary[] = SEEDS.map((seed) => ({
  id: seed.id,
  term: seed.term,
  phonetic: seed.phonetic,
  pos: seed.pos,
  difficulty: seed.difficulty,
  level: seed.level,
  meanings: seed.meanings,
  examples: seed.examples.map((ex, i) => ({ id: `${seed.id}_ex${i + 1}`, ...ex })),
  relatedWordIds: seed.related.filter((id) => SEEDS.some((s) => s.id === id)),
  grammarIds: seed.grammar,
  contexts: seed.contexts,
  addedAt: hintById.get(seed.id)?.addedAt ?? daysAgo(seed.addedDaysAgo, 21, 10),
}));

export const vocabularyIndex = new Map(vocabularySeed.map((w) => [w.id, w]));

export function findWord(id: string) {
  return vocabularyIndex.get(id);
}

/** 待复习单词：按逾期程度与记忆强度排序（到期信息取自学习态，不再读内容字段） */
export function dueWords(): Vocabulary[] {
  const now = Date.now();
  return wordStateHints
    .filter((hint) => hint.dueAt && new Date(hint.dueAt).getTime() <= now)
    .sort((left, right) => left.strength - right.strength)
    .map((hint) => vocabularyIndex.get(hint.entityId))
    .filter((word): word is Vocabulary => Boolean(word));
}

export function allWords() {
  return vocabularySeed;
}
