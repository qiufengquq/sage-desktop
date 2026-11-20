import type {
  Difficulty,
  EntityStateSeed,
  GrammarCategory,
  GrammarPoint,
  MasteryState,
} from "@/types";
import { daysAgo, daysAhead } from "./base";

interface GrammarSeed {
  id: string;
  title: string;
  titleEn: string;
  category: GrammarCategory;
  difficulty: Difficulty;
  level: string;
  summary: string;
  structure: string;
  explanation: string;
  examples: { en: string; zh: string; note?: string }[];
  mistakes: { wrong: string; correct: string; reason: string }[];
  related: string[];
  mastery: number;
  state: MasteryState;
  reviewCount: number;
  dueInDays: number;
  lastReviewDaysAgo: number;
}

const SEEDS: GrammarSeed[] = [
  {
    id: "g_present_perfect",
    title: "现在完成时",
    titleEn: "Present Perfect",
    category: "tense",
    difficulty: 3,
    level: "B1",
    summary: "强调过去发生的事与现在的联系，而不是动作本身的时间。",
    structure: "主语 + have/has + 过去分词",
    explanation:
      "现在完成时把注意力放在「结果或经历」上。与一般过去时不同，它不交代具体时间点，因此常与 already、yet、ever、since、for 连用。当句子出现明确的过去时间（yesterday、in 2020）时，必须改用一般过去时。",
    examples: [
      { en: "I have reviewed this list three times.", zh: "这份清单我已经复习过三遍。", note: "强调经历" },
      { en: "She has lived in Shanghai since 2019.", zh: "她自 2019 年起就住在上海。", note: "since + 时间点" },
      { en: "The app has become much faster.", zh: "这个应用已经快了很多。", note: "强调结果" },
    ],
    mistakes: [
      {
        wrong: "I have seen him yesterday.",
        correct: "I saw him yesterday.",
        reason: "yesterday 是明确过去时间，需用一般过去时。",
      },
      {
        wrong: "He has went to the library.",
        correct: "He has gone to the library.",
        reason: "have/has 后必须接过去分词，go 的过去分词是 gone。",
      },
    ],
    related: ["w_accumulate", "w_ubiquitous"],
    mastery: 74,
    state: "reviewing",
    reviewCount: 6,
    dueInDays: 2,
    lastReviewDaysAgo: 3,
  },
  {
    id: "g_past_simple",
    title: "一般过去时",
    titleEn: "Past Simple",
    category: "tense",
    difficulty: 1,
    level: "A2",
    summary: "描述在过去某个明确时间发生并结束的动作。",
    structure: "主语 + 动词过去式",
    explanation:
      "一般过去时需要一个可识别的时间参照：yesterday、last week、two days ago、when I was a child。它关注「动作发生过」，不关注对现在的影响。",
    examples: [
      { en: "She contemplated the question for a while.", zh: "她思考了这个问题一会儿。" },
      { en: "We started using this method last month.", zh: "我们上个月开始使用这个方法。" },
    ],
    mistakes: [
      {
        wrong: "Did you went there?",
        correct: "Did you go there?",
        reason: "did 已承担过去时标记，主要动词回到原形。",
      },
    ],
    related: ["w_contemplate", "w_deliberate"],
    mastery: 92,
    state: "mastered",
    reviewCount: 10,
    dueInDays: 20,
    lastReviewDaysAgo: 12,
  },
  {
    id: "g_relative_clause",
    title: "定语从句",
    titleEn: "Relative Clause",
    category: "clause",
    difficulty: 4,
    level: "B2",
    summary: "用 who/which/that/whose/where 把一句话变成名词的修饰成分。",
    structure: "名词 + 关系词 + 从句",
    explanation:
      "限定性定语从句紧跟先行词，不用逗号；非限定性定语从句用逗号隔开，且不能用 that。关系词在从句中作宾语时可以省略，作主语时不可省略。",
    examples: [
      { en: "The book that changed my mind was quite short.", zh: "那本改变了我想法的书其实很薄。", note: "that 作主语" },
      { en: "My tutor, who lives in Leeds, records every session.", zh: "我的导师住在利兹，他会录下每节课。", note: "非限定性，用 who" },
    ],
    mistakes: [
      {
        wrong: "The man which called you is my uncle.",
        correct: "The man who called you is my uncle.",
        reason: "指人时用 who（作宾语可用 whom/that），不用 which。",
      },
      {
        wrong: "Beijing, that is the capital, is cold in winter.",
        correct: "Beijing, which is the capital, is cold in winter.",
        reason: "非限定性定语从句不能用 that。",
      },
    ],
    related: ["w_perceive", "w_ambiguous"],
    mastery: 38,
    state: "learning",
    reviewCount: 2,
    dueInDays: 0,
    lastReviewDaysAgo: 5,
  },
  {
    id: "g_passive_voice",
    title: "被动语态",
    titleEn: "Passive Voice",
    category: "voice",
    difficulty: 3,
    level: "B1",
    summary: "动作承受者作主语，强调「被怎样」，弱化执行者。",
    structure: "主语 + be + 过去分词 (+ by ...)",
    explanation:
      "学术与工作场景中，被动语态让表达更客观。be 的时态决定整句时间，过去分词决定动作。只有及物动词可以变被动。",
    examples: [
      { en: "The proposal was rejected without public scrutiny.", zh: "该提案未经公众审查就被否决了。" },
      { en: "New words are added to the deck every morning.", zh: "每天早上都有新词被加入卡组。" },
    ],
    mistakes: [
      {
        wrong: "The problem was happen last night.",
        correct: "The problem happened last night.",
        reason: "happen 是不及物动词，没有被动语态。",
      },
    ],
    related: ["w_scrutiny", "w_inevitable"],
    mastery: 61,
    state: "reviewing",
    reviewCount: 4,
    dueInDays: 3,
    lastReviewDaysAgo: 8,
  },
  {
    id: "g_comparative",
    title: "比较级与最高级",
    titleEn: "Comparatives & Superlatives",
    category: "sentence-structure",
    difficulty: 2,
    level: "A2",
    summary: "比较两者用 -er/more，比较三者及以上用 -est/most。",
    structure: "A + 比较级 + than + B；the + 最高级 + in/of 范围",
    explanation:
      "单音节词通常加 -er/-est，三音节及以上用 more/most。避免双重比较（more better），也避免在同一句里既用 than 又用最高级。",
    examples: [
      { en: "Reading aloud is more effective than reading silently.", zh: "朗读比默读更有效。" },
      { en: "This is the hardest chapter in the book.", zh: "这是全书最难的一章。" },
    ],
    mistakes: [
      {
        wrong: "This method is more better.",
        correct: "This method is better.",
        reason: "better 本身已是比较级，不能叠加 more。",
      },
    ],
    related: ["w_fragile", "w_diligent", "w_resilient"],
    mastery: 87,
    state: "mastered",
    reviewCount: 8,
    dueInDays: 14,
    lastReviewDaysAgo: 9,
  },
  {
    id: "g_modal_verbs",
    title: "情态动词",
    titleEn: "Modal Verbs",
    category: "modality",
    difficulty: 3,
    level: "B1",
    summary: "can/may/must/should 表达能力、许可、推测与义务，后接动词原形。",
    structure: "主语 + 情态动词 + 动词原形",
    explanation:
      "情态动词不随人称变化，也没有 to。表达推测时，must 表示「一定」，can't 表示「不可能」，may/might 表示「也许」。过去推测用 情态动词 + have + 过去分词。",
    examples: [
      { en: "Small changes can mitigate the damage.", zh: "小的改变可以减轻损害。" },
      { en: "She must have forgotten the deadline.", zh: "她一定是忘了截止日期。" },
    ],
    mistakes: [
      {
        wrong: "He cans swim very well.",
        correct: "He can swim very well.",
        reason: "情态动词没有人称和数的变化。",
      },
    ],
    related: ["w_mitigate"],
    mastery: 44,
    state: "learning",
    reviewCount: 3,
    dueInDays: 1,
    lastReviewDaysAgo: 4,
  },
  {
    id: "g_article_definite",
    title: "冠词的使用",
    titleEn: "Articles",
    category: "article",
    difficulty: 2,
    level: "A2",
    summary: "a/an 引入一个新的可数名词，the 指向双方已知的那一个。",
    structure: "a/an + 可数名词单数；the + 特指名词",
    explanation:
      "第一次提到用 a/an，再次提到用 the。独一无二的事物、序数词、最高级前用 the。表示泛指复数的可数名词与不可数名词前不加冠词。",
    examples: [
      { en: "She wrote a paper on the effects of sleep.", zh: "她写了一篇关于睡眠影响的论文。" },
      { en: "The sun rose before we reached the station.", zh: "我们到车站前太阳就升起来了。" },
    ],
    mistakes: [
      {
        wrong: "He is best student in class.",
        correct: "He is the best student in the class.",
        reason: "最高级前需加 the；class 在此为特指。",
      },
    ],
    related: ["w_profound"],
    mastery: 52,
    state: "reviewing",
    reviewCount: 4,
    dueInDays: 4,
    lastReviewDaysAgo: 6,
  },
  {
    id: "g_infinitive",
    title: "动词不定式",
    titleEn: "Infinitives",
    category: "sentence-structure",
    difficulty: 3,
    level: "B1",
    summary: "to + 动词原形，可在句中作主语、宾语、目的状语或补语。",
    structure: "to + 动词原形",
    explanation:
      "部分动词后只能接不定式（decide、hope、refuse、be reluctant to），部分只能接动名词（enjoy、avoid、finish）。表示目的时，不定式前不加 for。",
    examples: [
      { en: "He was reluctant to admit the mistake.", zh: "他不愿承认那个错误。" },
      { en: "We need time to sustain this routine.", zh: "我们需要时间来维持这个习惯。" },
    ],
    mistakes: [
      {
        wrong: "I look forward to see you.",
        correct: "I look forward to seeing you.",
        reason: "look forward to 中的 to 是介词，后接动名词。",
      },
    ],
    related: ["w_reluctant", "w_sustain"],
    mastery: 67,
    state: "reviewing",
    reviewCount: 5,
    dueInDays: 2,
    lastReviewDaysAgo: 2,
  },
  {
    id: "g_prepositional_phrase",
    title: "介词短语",
    titleEn: "Prepositional Phrases",
    category: "preposition",
    difficulty: 3,
    level: "B1",
    summary: "介词 + 名词/动名词，用来补充时间、地点、原因与方式。",
    structure: "介词 + 名词短语 / 动名词",
    explanation:
      "固定搭配需要整体记忆：in light of、on behalf of、regardless of、in terms of。介词后接动词时必须用动名词。",
    examples: [
      { en: "In light of the results, we changed the plan.", zh: "鉴于这些结果，我们改变了计划。" },
      { en: "Take your energy level into account before scheduling.", zh: "安排时间前，先把你的精力水平考虑进去。" },
    ],
    mistakes: [
      {
        wrong: "In light of improve the accuracy...",
        correct: "In light of improving the accuracy...",
        reason: "介词后接名词性成分，动词需变为动名词。",
      },
    ],
    related: ["w_in_light_of", "w_take_into_account"],
    mastery: 29,
    state: "learning",
    reviewCount: 1,
    dueInDays: 0,
    lastReviewDaysAgo: 3,
  },
  {
    id: "g_sentence_structure",
    title: "句子结构与连接",
    titleEn: "Sentence Structure",
    category: "sentence-structure",
    difficulty: 4,
    level: "B2",
    summary: "避免逗号粘连与流水句，用连接词标明分句之间的逻辑关系。",
    structure: "主句 + 连接词 + 分句（或使用分号 / 关系从句）",
    explanation:
      "两个完整句子不能只用逗号连接。可用并列连词（and、but、so）、从属连词（because、although、while）或分号。学术写作中，逻辑连接词要和内容真实对应，不能滥用。",
    examples: [
      { en: "The data was incomplete; nevertheless, the trend was clear.", zh: "数据并不完整；尽管如此，趋势是清楚的。" },
      { en: "Although the task was short, it required full attention.", zh: "虽然任务很短，但需要全神贯注。" },
    ],
    mistakes: [
      {
        wrong: "The plan was good, it failed anyway.",
        correct: "The plan was good, but it failed anyway.",
        reason: "两个完整句之间需要连词或分号。",
      },
    ],
    related: ["w_coherent"],
    mastery: 47,
    state: "learning",
    reviewCount: 2,
    dueInDays: 5,
    lastReviewDaysAgo: 7,
  },
];

/** 初始学习态种子（与内容正文分离，仅首次建库使用） */
export const grammarStateHints: EntityStateSeed[] = SEEDS.map((seed) => ({
  entityId: seed.id,
  kind: "grammar",
  mastery: seed.mastery,
  strength: seed.mastery / 100,
  state: seed.state,
  reviewCount: seed.reviewCount,
  dueAt: daysAhead(seed.dueInDays, 9, 0),
  lastReviewedAt: daysAgo(seed.lastReviewDaysAgo, 20, 15),
  addedAt: null,
}));

export const grammarSeed: GrammarPoint[] = SEEDS.map((seed) => ({
  id: seed.id,
  title: seed.title,
  titleEn: seed.titleEn,
  category: seed.category,
  difficulty: seed.difficulty,
  level: seed.level,
  summary: seed.summary,
  structure: seed.structure,
  explanation: seed.explanation,
  examples: seed.examples.map((ex, i) => ({ id: `${seed.id}_ex${i + 1}`, ...ex })),
  mistakes: seed.mistakes.map((m, i) => ({ id: `${seed.id}_m${i + 1}`, ...m })),
  relatedWordIds: seed.related,
}));

export const grammarIndex = new Map(grammarSeed.map((g) => [g.id, g]));

export function findGrammar(id: string) {
  return grammarIndex.get(id);
}

export function allGrammar() {
  return grammarSeed;
}
