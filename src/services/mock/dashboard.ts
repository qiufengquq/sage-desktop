import type { DashboardData, DashboardTask, TrainingOption } from "@/types";
import { trainingOptions } from "./exercises";
import { learningRecords, reviewQueue, snapshot } from "./learning";
import { studyPlan } from "./planner";

const tasks: DashboardTask[] = studyPlan.today.items.map((item) => ({
  id: item.id,
  title: item.title,
  activity: item.activity,
  minutes: item.minutes,
  xp: item.xp,
  reason: item.reason,
  status: item.status,
  action: item.action,
}));

const quickStartPool: Record<string, TrainingOption | undefined> = Object.fromEntries(
  trainingOptions.map((o) => [o.kind + o.track, o]),
);

const quickStart = [
  quickStartPool["multiple-choicevocabulary"],
  quickStartPool["error-correctiongrammar"],
  quickStartPool["shadowingspeaking"],
].filter((o): o is TrainingOption => Boolean(o));

export const dashboardData: DashboardData = {
  snapshot,
  tasks,
  reviewTasks: reviewQueue.slice(0, 5),
  recentRecords: learningRecords.slice(0, 5),
  recommendation: studyPlan.advice[0] ?? null,
  quickStart,
  planCompletion: studyPlan.completion.rate,
  nowProgress: (() => {
    const now = new Date();
    return (now.getHours() * 60 + now.getMinutes()) / (24 * 60);
  })(),
};
