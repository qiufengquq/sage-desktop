import type {
  AbilityAxis,
  AbilityProfile,
  PlanDay,
  PlanItem,
  StudyPlan,
  WeeklyGoal,
} from "@/types";
import { addDays, isoDate } from "@/lib/date";

const AXES: AbilityAxis[] = [
  {
    key: "vocabulary",
    label: "词汇量",
    score: 68,
    benchmark: 60,
    level: "B2",
    confidence: "measured",
    source: "词汇测评 68 分",
  },
  {
    key: "grammar",
    label: "语法准确度",
    score: 54,
    benchmark: 58,
    level: "B1",
    confidence: "estimated",
    source: "基线估值，等待首次语法测评",
  },
  {
    key: "listening",
    label: "听力理解",
    score: 71,
    benchmark: 62,
    level: "B2",
    confidence: "estimated",
    source: "听写 / 跟读题正确率",
  },
  {
    key: "speaking",
    label: "口语表达",
    score: 47,
    benchmark: 55,
    level: "B1",
    confidence: "estimated",
    source: "基线估值，等待首次口语测评",
  },
  {
    key: "reading",
    label: "阅读理解",
    score: 76,
    benchmark: 64,
    level: "B2",
    confidence: "estimated",
    source: "完形 / 翻译题正确率",
  },
  {
    key: "writing",
    label: "写作",
    score: 58,
    benchmark: 56,
    level: "B1",
    confidence: "estimated",
    source: "改写 / 翻译题正确率",
  },
];

export const abilityProfile: AbilityProfile = {
  level: "B1",
  targetLevel: "B2",
  overall: 62,
  axes: AXES,
  strengths: ["阅读理解高于同水平 12 分", "听力理解稳定在 B2 区间"],
  weaknesses: ["主动表达明显滞后于理解能力", "从句结构错误集中在关系词选择"],
  updatedAt: addDays(new Date(), -2).toISOString(),
  sourceAssessmentId: "as_vocab_3",
};

const TODAY = isoDate(new Date());

function items(dayIndex: number, focus: string): PlanItem[] {
  const base: Omit<PlanItem, "id" | "done" | "status">[] = [
    {
      title: "复习到期词汇",
      activity: "vocabulary",
      minutes: 10,
      action: { type: "training", kind: "multiple-choice", track: "vocabulary" },
      xp: 24,
      reason: "有 6 个词已逾期，逾期时间越长，重学成本越高。",
    },
    {
      title: focus,
      activity: dayIndex % 2 === 0 ? "grammar" : "speaking",
      minutes: 12,
      action:
        dayIndex % 2 === 0
          ? { type: "training", kind: "error-correction", track: "grammar" }
          : { type: "training", kind: "shadowing", track: "speaking" },
      xp: 30,
      reason:
        dayIndex % 2 === 0
          ? "最近 3 次语法纠错中，关系词错误占 62%。"
          : "口语是当前画像中最弱的一项，需要高频短时训练。",
    },
    {
      title: "新词入队",
      activity: "vocabulary",
      minutes: 8,
      action: { type: "training", kind: "spelling", track: "vocabulary" },
      xp: 20,
      reason: "按你每日 40 分钟的目标，新词速度控制在 8 个/天最稳。",
    },
  ];

  return base.map((item, i) => {
    const isPast = dayIndex < 0;
    const isToday = dayIndex === 0;
    const done = isPast || (isToday && i === 0);
    const inProgress = isToday && i === 1;
    return {
      ...item,
      id: `pi_${isoDate(addDays(new Date(), dayIndex))}_${i}`,
      done,
      status: done ? "done" : inProgress ? "in-progress" : "pending",
    };
  });
}

const WEEK_FOCUS = [
  "定语从句纠错",
  "学术跟读 3 句",
  "第二条件句改写",
  "情景对话 · 会议",
  "介词短语填空",
  "句子排序 · 长句",
  "本周复盘与测试",
];

const todayItems = items(0, "定语从句纠错");

export const studyPlan: StudyPlan = {
  id: "plan_2026_w46",
  profile: abilityProfile,
  today: {
    id: `pd_${TODAY}`,
    date: TODAY,
    focus: "定语从句 + 逾期词汇",
    minutes: todayItems.reduce((acc, i) => acc + i.minutes, 0),
    items: todayItems,
    completed: false,
  },
  week: WEEK_FOCUS.map((focus, i) => {
    const dayItems = items(i === 0 ? 0 : i, focus);
    return {
      id: `pd_${isoDate(addDays(new Date(), i))}`,
      date: isoDate(addDays(new Date(), i)),
      focus,
      minutes: dayItems.reduce((acc, it) => acc + it.minutes, 0),
      items: dayItems,
      completed: false,
    } as PlanDay;
  }),
  weeklyGoals: [
    {
      id: "wg_minutes",
      label: "本周学习时长",
      target: 240,
      current: 156,
      unit: "分钟",
      rationale: "按每日 40 分钟 × 6 天设定，留 1 天缓冲。",
    },
    {
      id: "wg_days",
      label: "本周学习天数",
      target: 6,
      current: 4,
      unit: "天",
      rationale: "你过去 4 周的平均活跃天数是 5.2 天。",
    },
    {
      id: "wg_words",
      label: "新掌握词汇",
      target: 40,
      current: 23,
      unit: "词",
      rationale: "以当前记忆强度推算，每周 40 词不会挤压复习时间。",
    },
    {
      id: "wg_items",
      label: "语法题正确数",
      target: 60,
      current: 38,
      unit: "题",
      rationale: "把语法准确度从 54 推到 62 所需的最小练习量。",
    },
  ] satisfies WeeklyGoal[],
  advice: [
    {
      id: "adv_vocab",
      kind: "vocabulary",
      title: "先清逾期，再加新词",
      body: "你有 6 个词逾期超过 2 天。建议今天只复习、不新增，明天再恢复 8 个新词/天。逾期词的重学成本约为新词的 2.4 倍。",
      evidence: [
        "逾期词汇 6 个，平均记忆强度 0.31",
        "过去 7 天新词 42 个，复习完成率 68%",
      ],
      action: { label: "开始复习过期词汇", trainingKind: "multiple-choice", track: "vocabulary" },
    },
    {
      id: "adv_grammar",
      kind: "grammar",
      title: "把关系词当词汇背",
      body: "你的从句错误 62% 集中在 who/which/that 的选择上，这是记忆型错误而非理解型错误。用「句子排序 + 纠错」交替训练，比单纯做选择题有效。",
      evidence: [
        "近 3 次语法训练共 34 题，关系词错误 21 题",
        "定语从句掌握度 38%，低于语法平均 54%",
      ],
      action: { label: "训练定语从句", trainingKind: "error-correction", track: "grammar" },
    },
    {
      id: "adv_speaking",
      kind: "speaking",
      title: "跟读优先于自由表达",
      body: "当前口语 47 分，主要问题是停顿过多而不是语法。先做 3 句短跟读建立节奏，再进入情景对话，每次不超过 8 分钟。",
      evidence: ["口语自评与文本相似度差距 0.34", "过去 30 天口语训练仅 3 次"],
      action: { label: "开始跟读训练", trainingKind: "shadowing", track: "speaking" },
    },
  ],
  difficulty: {
    current: 2.8,
    suggested: 3.2,
    reason:
      "近 10 次训练正确率 86%，高于目标区间 75–82%。适当提高难度可以维持挑战性，但只在你确认后生效。",
    requiresConfirmation: true,
  },
  completion: {
    done: todayItems.filter((i) => i.done).length,
    total: todayItems.length,
    rate: Math.round(
      (todayItems.filter((i) => i.done).length / Math.max(1, todayItems.length)) * 100,
    ),
  },
  generatedAt: new Date().toISOString(),
  model: "sage-planner-v1 (mock)",
};
