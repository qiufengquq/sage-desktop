export type CefrLevel = "A1" | "A2" | "B1" | "B2" | "C1" | "C2";

export type LearningGoal =
  | "exam"
  | "career"
  | "travel"
  | "academic"
  | "interest";

export interface UserProfile {
  id: string;
  displayName: string;
  handle: string;
  joinedAt: string;
  level: CefrLevel;
  targetLevel: CefrLevel;
  goal: LearningGoal;
  /** 每日目标学习分钟数 */
  dailyMinutes: number;
  /** 每周目标学习天数 */
  weeklyDays: number;
  timezone: string;
}

export type ThemeMode = "light" | "dark" | "system";
export type AccentMode = "brand" | "ember";

export interface AppearanceSettings {
  theme: ThemeMode;
  accent: AccentMode;
  /** 界面密度 */
  density: "comfortable" | "compact";
  reduceMotion: boolean;
}

export interface ReminderSettings {
  enabled: boolean;
  /** HH:mm */
  time: string;
  weekdaysOnly: boolean;
}

export interface AppSettings {
  appearance: AppearanceSettings;
  reminder: ReminderSettings;
  /** 学习时是否播放提示音 */
  soundEffects: boolean;
}

export type SyncState = "idle" | "syncing" | "synced" | "offline" | "error";

export interface SyncStatus {
  state: SyncState;
  lastSyncedAt: string | null;
  pendingChanges: number;
  deviceName: string;
  storageUsedKb: number;
}

export interface LocalDataSummary {
  words: number;
  grammarPoints: number;
  sentences: number;
  records: number;
  sizeKb: number;
}
