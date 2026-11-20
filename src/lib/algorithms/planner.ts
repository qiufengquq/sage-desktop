import type {
  AbilityAxis,
  AbilityAxisKey,
  AbilityProfile,
  EntityState,
  ExerciseKind,
  PlanAdvice,
  PlanDay,
  PlanItem,
  PlanItemAction,
  PlanGenerationMeta,
  StudyPlan,
  TrainingTrack,
  UserProfile,
  WeeklyGoal,
} from "@/types";
import { addDays, isoDate } from "@/lib/date";
import { clamp } from "@/lib/utils";

/**
 * 确定性学习计划生成器（不接 AI）。
 *
 * 输入全部来自真实业务数据：水平、目标、画像、掌握度、到期数量、最近正确率、每日时长。
 * 输出结构即前端契约，未来把这里换成 AI 时，只需要生成同样的 StudyPlan。
 *
 * 生成规则（可逐条核对）：
 * 1. 有到期复习就先复习：时长随到期数量线性增长，上限 15 分钟。
 * 2. 预算的 35% 投给当前最弱的能力轴，形成"专项"。
 * 3. 剩余预算用于新素材（拼写 / 听写）。
 * 4. 预算 ≥ 60 分钟时追加一段口语情景练习。
 * 5. 难度按最近 10 次训练正确率调整，目标区间 75–82%。
 */

const AXIS_TRAINING: Record<AbilityAxisKey, { kind: ExerciseKind; track: TrainingTrack }> = {
  vocabulary: { kind: "multiple-choice", track: "vocabulary" },
  grammar: { kind: "error-correction", track: "grammar" },
  speaking: { kind: "shadowing", track: "speaking" },
  listening: { kind: "dictation", track: "vocabulary" },
  reading: { kind: "cloze", track: "vocabulary" },
  writing: { kind: "rewrite", track: "grammar" },
};

const AXIS_ACTIVITY: Record<AbilityAxisKey, PlanItem["activity"]> = {
  vocabulary: "vocabulary",
  grammar: "grammar",
  speaking: "speaking",
  listening: "speaking",
  reading: "vocabulary",
  writing: "grammar",
};

export interface PlanGenerationInput {
  profile: UserProfile;
  ability: AbilityProfile;
  states: Record<string, EntityState>;
  dueCount: number;
  recentAccuracy: number | null;
  weeklyMinutes: number;
  weeklyActiveDays: number;
  weeklyNewMastered: number;
  weeklyGrammarCorrect: number;
  /** 用户手动确认过的难度，null 表示沿用建议值 */
  difficultyOverride: number | null;
  generatedBy: PlanGenerationMeta["generatedBy"];
  now: Date;
  /** 重新生成时保留今天已经完成的任务 */
  previous?: StudyPlan | null;
}

export function generatePlan(input: PlanGenerationInput): StudyPlan {
  const { profile, ability, now } = input;
  const budget = clamp(profile.dailyMinutes, 10, 120);
  const weakest = weakestAxis(ability);
  const training = AXIS_TRAINING[weakest.key];
  const today = isoDate(now);

  const items: PlanItem[] = [];
  let used = 0;

  if (input.dueCount > 0) {
    const minutes = clamp(Math.round(input.dueCount * 0.6), 5, 15);
    used += minutes;
    items.push({
      id: `pi_${today}_review`,
      title: `复习到期知识 ${input.dueCount} 项`,
      activity: "review",
      minutes,
      action: { type: "review" } satisfies PlanItemAction,
      done: false,
      status: "pending",
      xp: minutes * 2 + 6,
      reason: `复习队列里有 ${input.dueCount} 项已到复习时间，逾期越久重学成本越高。`,
    });
  }

  const focusMinutes = clamp(Math.round(budget * 0.35), 6, 20);
  used += focusMinutes;
  items.push({
    id: `pi_${today}_focus`,
    title: `弱点专项 · ${weakest.label}`,
    activity: AXIS_ACTIVITY[weakest.key],
    minutes: focusMinutes,
    action: { type: "training", kind: training.kind, track: training.track } satisfies PlanItemAction,
    done: false,
    status: "pending",
    xp: focusMinutes * 3,
    reason: `${weakest.label}当前 ${weakest.score} 分，是六项能力中最低的一项（${
      weakest.confidence === "measured" ? "测评结果" : "训练表现估算"
    }）。`,
  });

  const remaining = budget - used;
  if (remaining >= 5) {
    const minutes = Math.min(remaining, 20);
    items.push({
      id: `pi_${today}_new`,
      title: "新素材 · 拼写与听写",
      activity: "vocabulary",
      minutes,
      action: {
        type: "training",
        kind: profile.level >= "B2" ? "dictation" : "spelling",
        track: "vocabulary",
      } satisfies PlanItemAction,
      done: false,
      status: "pending",
      xp: minutes * 3,
      reason: `按每日 ${profile.dailyMinutes} 分钟的目标，剩余 ${minutes} 分钟用于巩固新词。`,
    });
  } else if (budget >= 60) {
    items.push({
      id: `pi_${today}_speaking`,
      title: "口语 · 情景对话",
      activity: "speaking",
      minutes: 8,
      action: { type: "training", kind: "roleplay", track: "speaking" } satisfies PlanItemAction,
      done: false,
      status: "pending",
      xp: 24,
      reason: "本次预算较高，追加一段低频高价值的输出练习。",
    });
  }

  const withProgress = carryOverDone(items, input.previous);
  const done = withProgress.filter((item) => item.done).length;
  const difficulty = computeDifficulty(input);
  const week = buildWeek(today, budget, weakest, items);

  return {
    id: `plan_${today}`,
    profile: ability,
    today: {
      id: `pd_${today}`,
      date: today,
      focus: buildFocus(weakest, input.dueCount),
      minutes: withProgress.reduce((acc, item) => acc + item.minutes, 0),
      items: withProgress,
      completed: done === withProgress.length,
    },
    week,
    weeklyGoals: buildWeeklyGoals(input),
    advice: buildAdvice(input, weakest, training),
    difficulty,
    completion: {
      done,
      total: withProgress.length,
      rate: Math.round((done / Math.max(1, withProgress.length)) * 100),
    },
    generatedAt: now.toISOString(),
    model: "sage-planner-v1 (rule-based)",
  };
}

export function weakestAxis(ability: AbilityProfile): AbilityAxis {
  return [...ability.axes].sort((a, b) => a.score - b.score)[0] as AbilityAxis;
}

function computeDifficulty(input: PlanGenerationInput): StudyPlan["difficulty"] {
  const baseline = clamp(1 + input.ability.overall / 25, 1.5, 4.5);
  const accuracy = input.recentAccuracy;
  let suggested = baseline;
  let reason: string;

  if (accuracy === null) {
    reason = "还没有最近训练数据，先按能力画像给出起始难度，完成后会自动调整。";
  } else if (accuracy > 85) {
    suggested = baseline + 0.3;
    reason = `最近训练正确率 ${accuracy}%，高于目标区间 75–82%，可以提高一档。`;
  } else if (accuracy < 70) {
    suggested = baseline - 0.3;
    reason = `最近训练正确率 ${accuracy}%，低于目标区间 75–82%，先降低难度稳住正确率。`;
  } else {
    reason = `最近训练正确率 ${accuracy}%，处于目标区间 75–82%，保持当前难度。`;
  }

  const suggestedRounded = Math.round(clamp(suggested, 1, 5) * 10) / 10;
  const current = input.difficultyOverride ?? suggestedRounded;

  return {
    current: Math.round(current * 10) / 10,
    suggested: suggestedRounded,
    reason,
    requiresConfirmation: input.difficultyOverride === null,
  };
}

function buildWeeklyGoals(input: PlanGenerationInput): WeeklyGoal[] {
  return [
    {
      id: "wg_minutes",
      label: "本周学习时长",
      target: input.profile.dailyMinutes * input.profile.weeklyDays,
      current: Math.round(input.weeklyMinutes),
      unit: "分钟",
      rationale: `每日 ${input.profile.dailyMinutes} 分钟 × 每周 ${input.profile.weeklyDays} 天。`,
    },
    {
      id: "wg_days",
      label: "本周学习天数",
      target: input.profile.weeklyDays,
      current: input.weeklyActiveDays,
      unit: "天",
      rationale: `按你设置的每周 ${input.profile.weeklyDays} 天目标。`,
    },
    {
      id: "wg_words",
      label: "新掌握词汇",
      target: Math.max(8, Math.round(input.profile.dailyMinutes / 5)),
      current: input.weeklyNewMastered,
      unit: "词",
      rationale: `以每日 ${input.profile.dailyMinutes} 分钟推算的新增速度。`,
    },
    {
      id: "wg_items",
      label: "语法题正确数",
      target: 40,
      current: input.weeklyGrammarCorrect,
      unit: "题",
      rationale: "把语法准确度推高一档所需的最小练习量。",
    },
  ];
}

function buildAdvice(
  input: PlanGenerationInput,
  weakest: AbilityAxis,
  training: { kind: ExerciseKind; track: TrainingTrack },
): PlanAdvice[] {
  const advice: PlanAdvice[] = [];

  if (input.dueCount > 0) {
    advice.push({
      id: "adv_review",
      kind: "vocabulary",
      title: "先清到期，再学新内容",
      body: `复习队列有 ${input.dueCount} 项已到期。先复习再学新词，能避免"学了新的、忘了旧的"。`,
      evidence: [
        `到期条目 ${input.dueCount} 项`,
        `词汇掌握度平均值 ${averageMasteryOf(input.states, ["word", "phrase"])} 分`,
      ],
      action: { label: "开始复习", trainingKind: "multiple-choice", track: "vocabulary" },
    });
  }

  advice.push({
    id: "adv_weakest",
    kind: weakest.key === "speaking" ? "speaking" : weakest.key === "grammar" ? "grammar" : "vocabulary",
    title: `把最短的一块补上：${weakest.label}`,
    body: `${weakest.label}当前 ${weakest.score} 分，与参考均值差 ${
      weakest.benchmark - weakest.score
    } 分。先做一组针对性训练，比漫无目的地刷题有效。`,
    evidence: [
      `来源：${weakest.source}`,
      `参考均值 ${weakest.benchmark} 分，当前 ${weakest.score} 分`,
    ],
    action: { label: `训练${weakest.label}`, trainingKind: training.kind, track: training.track },
  });

  const accuracy = input.recentAccuracy;
  advice.push({
    id: "adv_difficulty",
    kind: "grammar",
    title: accuracy === null ? "先用一组训练校准难度" : accuracy > 85 ? "该提高难度了" : accuracy < 70 ? "先把正确率稳住" : "保持节奏，逐步加量",
    body:
      accuracy === null
        ? "还没有足够的训练记录，完成一组训练后系统会自动给出难度建议。"
        : `最近 10 次训练正确率 ${accuracy}%，目标区间是 75–82%。难度会随表现自动调整，也可以在规划页手动确认。`,
    evidence:
      accuracy === null
        ? ["训练记录：0 次"]
        : [`最近正确率 ${accuracy}%`, `本周已完成 ${Math.round(input.weeklyMinutes)} 分钟`],
    action: {
      label: "去训练",
      trainingKind: accuracy !== null && accuracy > 85 ? "rewrite" : "multiple-choice",
      track: accuracy !== null && accuracy > 85 ? "grammar" : "vocabulary",
    },
  });

  return advice;
}

function buildFocus(weakest: AbilityAxis, dueCount: number): string {
  const parts = [weakest.label];
  if (dueCount > 0) parts.push(`到期复习 ${dueCount} 项`);
  return parts.join(" + ");
}

function buildWeek(
  today: string,
  budget: number,
  weakest: AbilityAxis,
  todayItems: PlanItem[],
): PlanDay[] {
  const base = new Date(`${today}T09:00:00`);
  return Array.from({ length: 7 }, (_, i) => {
    const date = isoDate(addDays(base, i));
    return {
      id: `pd_${date}`,
      date,
      focus: i === 0 ? buildFocus(weakest, 0) : `${weakest.label} · 待当天生成`,
      minutes: i === 0 ? todayItems.reduce((acc, item) => acc + item.minutes, 0) : budget,
      items: i === 0 ? todayItems : [],
      completed: false,
    };
  });
}

/** 重新生成计划时，把今天已完成的任务状态保留下来（按 action 身份匹配） */
function carryOverDone(items: PlanItem[], previous: StudyPlan | null | undefined): PlanItem[] {
  if (!previous) return items;
  const doneKeys = new Set(
    previous.today.items
      .filter((item) => item.done)
      .map((item) => `${item.action.type}:${item.action.type === "training" ? item.action.kind : ""}`),
  );
  return items.map((item) => {
    const key = `${item.action.type}:${item.action.type === "training" ? item.action.kind : ""}`;
    return doneKeys.has(key)
      ? { ...item, done: true, status: "done" as const }
      : item;
  });
}

function averageMasteryOf(states: Record<string, EntityState>, kinds: EntityState["kind"][]) {
  const list = Object.values(states).filter((s) => kinds.includes(s.kind));
  if (list.length === 0) return 0;
  return Math.round(list.reduce((acc, s) => acc + s.mastery, 0) / list.length);
}
