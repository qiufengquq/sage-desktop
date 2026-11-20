import type { AppSettings, LocalDataSummary, SyncStatus, UserProfile } from "@/types";
import { daysAgo } from "./base";
import { allGrammar } from "./grammar";
import { allSentences } from "./sentences";
import { allWords } from "./vocabulary";
import { dailyStats } from "./learning";

export const userProfile: UserProfile = {
  id: "u_local_01",
  displayName: "林一舟",
  handle: "@yizhou",
  joinedAt: daysAgo(184, 20, 12),
  level: "B1",
  targetLevel: "B2",
  goal: "exam",
  dailyMinutes: 40,
  weeklyDays: 6,
  timezone: "Asia/Shanghai",
};

export const defaultSettings: AppSettings = {
  appearance: {
    theme: "system",
    accent: "brand",
    density: "comfortable",
    reduceMotion: false,
  },
  reminder: {
    enabled: true,
    time: "20:30",
    weekdaysOnly: false,
  },
  soundEffects: true,
};

export function syncStatus(): SyncStatus {
  return {
    state: "synced",
    lastSyncedAt: daysAgo(0, 8, 42),
    pendingChanges: 0,
    deviceName: "DESKTOP-SAGE",
    storageUsedKb: 2840,
  };
}

export function localDataSummary(): LocalDataSummary {
  const words = allWords().length;
  const grammarPoints = allGrammar().length;
  const sentences = allSentences().length;
  const records = dailyStats.filter((d) => d.minutes > 0).length;
  return {
    words,
    grammarPoints,
    sentences,
    records,
    sizeKb: Math.round(words * 4.2 + grammarPoints * 6.8 + sentences * 5.1 + 640),
  };
}
