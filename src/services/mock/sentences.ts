import type {
  Difficulty,
  EntityStateSeed,
  MasteryState,
  Sentence,
  SentenceCategory,
} from "@/types";
import { daysAgo, daysAhead } from "./base";

interface SentenceSeed {
  id: string;
  en: string;
  zh: string;
  category: SentenceCategory;
  difficulty: Difficulty;
  level: string;
  grammar: string[];
  words: string[];
  chunks: string[];
  mastery: number;
  state: MasteryState;
  reviewCount: number;
  dueInDays: number;
  lastReviewDaysAgo: number;
}

const SEEDS: SentenceSeed[] = [
  {
    id: "s_small_habits",
    en: "Small habits accumulate into visible progress, which is why consistency matters more than intensity.",
    zh: "微小的习惯会积累成可见的进步，这正是坚持比强度更重要的原因。",
    category: "opinion",
    difficulty: 4,
    level: "B2",
    grammar: ["g_relative_clause", "g_comparative"],
    words: ["w_accumulate", "w_diligent"],
    chunks: ["Small habits accumulate", "into visible progress", "which is why consistency matters", "more than intensity"],
    mastery: 58,
    state: "learning",
    reviewCount: 3,
    dueInDays: 1,
    lastReviewDaysAgo: 4,
  },
  {
    id: "s_meeting_shift",
    en: "In light of the new data, we have decided to move the meeting to Thursday afternoon.",
    zh: "鉴于新的数据，我们已决定把会议改到周四下午。",
    category: "workplace",
    difficulty: 3,
    level: "B1",
    grammar: ["g_present_perfect", "g_prepositional_phrase"],
    words: ["w_in_light_of"],
    chunks: ["In light of the new data", "we have decided", "to move the meeting", "to Thursday afternoon"],
    mastery: 41,
    state: "reviewing",
    reviewCount: 4,
    dueInDays: 0,
    lastReviewDaysAgo: 5,
  },
  {
    id: "s_passive_study",
    en: "The study was designed to measure how quickly new vocabulary is forgotten without review.",
    zh: "这项研究旨在测量新词汇在没有复习的情况下被遗忘的速度。",
    category: "academic",
    difficulty: 4,
    level: "B2",
    grammar: ["g_passive_voice", "g_relative_clause"],
    words: ["w_perceive", "w_scrutiny"],
    chunks: ["The study was designed", "to measure how quickly", "new vocabulary is forgotten", "without review"],
    mastery: 33,
    state: "learning",
    reviewCount: 2,
    dueInDays: 2,
    lastReviewDaysAgo: 6,
  },
  {
    id: "s_travel_checkin",
    en: "Could you tell me whether the train to the airport leaves from this platform?",
    zh: "你能告诉我开往机场的火车是不是从这个站台出发吗？",
    category: "travel",
    difficulty: 2,
    level: "A2",
    grammar: ["g_relative_clause", "g_modal_verbs"],
    words: [],
    chunks: ["Could you tell me", "whether the train to the airport", "leaves from this platform"],
    mastery: 86,
    state: "mastered",
    reviewCount: 7,
    dueInDays: 16,
    lastReviewDaysAgo: 10,
  },
  {
    id: "s_coffee_small_talk",
    en: "I was going to grab a coffee before the session — do you want to join me?",
    zh: "我本来打算在上课前买杯咖啡——你要一起吗？",
    category: "daily",
    difficulty: 2,
    level: "A2",
    grammar: ["g_past_simple", "g_infinitive"],
    words: [],
    chunks: ["I was going to grab a coffee", "before the session", "do you want to join me"],
    mastery: 72,
    state: "reviewing",
    reviewCount: 5,
    dueInDays: 3,
    lastReviewDaysAgo: 3,
  },
  {
    id: "s_argument_quality",
    en: "A coherent argument does not need complicated words; it needs evidence at every step.",
    zh: "一个连贯的论点不需要复杂的词，它需要在每一步都有证据。",
    category: "opinion",
    difficulty: 4,
    level: "B2",
    grammar: ["g_sentence_structure", "g_article_definite"],
    words: ["w_coherent"],
    chunks: ["A coherent argument", "does not need complicated words", "it needs evidence", "at every step"],
    mastery: 64,
    state: "reviewing",
    reviewCount: 4,
    dueInDays: 4,
    lastReviewDaysAgo: 7,
  },
  {
    id: "s_mitigate_risk",
    en: "The team must mitigate the risk before the release, or the incident will repeat itself.",
    zh: "团队必须在发布前降低风险，否则事故会再次发生。",
    category: "workplace",
    difficulty: 4,
    level: "B2",
    grammar: ["g_modal_verbs", "g_sentence_structure"],
    words: ["w_mitigate"],
    chunks: ["The team must mitigate the risk", "before the release", "or the incident will repeat itself"],
    mastery: 26,
    state: "learning",
    reviewCount: 1,
    dueInDays: 1,
    lastReviewDaysAgo: 2,
  },
  {
    id: "s_profound_effect",
    en: "Reading fiction has a profound effect on how we understand other people.",
    zh: "阅读小说深刻地影响我们理解他人的方式。",
    category: "academic",
    difficulty: 3,
    level: "B1",
    grammar: ["g_present_perfect", "g_article_definite"],
    words: ["w_profound"],
    chunks: ["Reading fiction", "has a profound effect", "on how we understand other people"],
    mastery: 49,
    state: "learning",
    reviewCount: 2,
    dueInDays: 0,
    lastReviewDaysAgo: 5,
  },
];

/** 初始学习态种子（与内容正文分离，仅首次建库使用） */
export const sentenceStateHints: EntityStateSeed[] = SEEDS.map((seed) => ({
  entityId: seed.id,
  kind: "sentence",
  mastery: seed.mastery,
  strength: seed.mastery / 100,
  state: seed.state,
  reviewCount: seed.reviewCount,
  dueAt: daysAhead(seed.dueInDays, 8, 30),
  lastReviewedAt: daysAgo(seed.lastReviewDaysAgo, 19, 40),
  addedAt: null,
}));

export const sentenceSeed: Sentence[] = SEEDS.map((seed) => ({
  id: seed.id,
  en: seed.en,
  zh: seed.zh,
  category: seed.category,
  difficulty: seed.difficulty,
  level: seed.level,
  grammarIds: seed.grammar,
  wordIds: seed.words,
  chunks: seed.chunks,
}));

export const sentenceIndex = new Map(sentenceSeed.map((s) => [s.id, s]));

export function findSentence(id: string) {
  return sentenceIndex.get(id);
}

export function allSentences() {
  return sentenceSeed;
}
