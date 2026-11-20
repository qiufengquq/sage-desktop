import type {
  ApiResult,
  AppSettings,
  LocalDataSummary,
  RequestOptions,
  SyncStatus,
  UserProfile,
} from "@/types";
import { latency } from "@/services/mock/base";
import { localDataSummaryOf } from "@/services/data/selectors";
import { clearLearningRecords, rebuildPlan } from "@/services/data/learning-db";
import { commitDb, getDb } from "@/stores/learning-store";
import { COMMANDS } from "./contracts";
import { serve } from "./transport";

export function getProfile(options: RequestOptions = {}): Promise<ApiResult<UserProfile>> {
  return serve(
    COMMANDS.userProfileGet,
    async () => {
      await latency(options.signal);
      return getDb().profile;
    },
    { request: options },
  );
}

export function updateProfile(patch: Partial<UserProfile>): Promise<ApiResult<UserProfile>> {
  return serve(
    COMMANDS.userProfileUpdate,
    async () => {
      await latency();
      const next = commitDb((db) => {
        const profile = { ...db.profile, ...patch };
        // 每日预算与每周天数会影响计划，资料变化后按新预算重排
        const changedPlanInput =
          profile.dailyMinutes !== db.profile.dailyMinutes ||
          profile.weeklyDays !== db.profile.weeklyDays ||
          profile.level !== db.profile.level ||
          profile.targetLevel !== db.profile.targetLevel;
        const withProfile = { ...db, profile };
        return changedPlanInput
          ? rebuildPlan(withProfile, { reason: "regenerate", now: new Date() })
          : withProfile;
      });
      return next.profile;
    },
    { args: { patch } },
  );
}

export function getSettings(options: RequestOptions = {}): Promise<ApiResult<AppSettings>> {
  return serve(
    COMMANDS.settingsGet,
    async () => {
      await latency(options.signal);
      return getDb().settings;
    },
    { request: options },
  );
}

export function updateSettings(patch: Partial<AppSettings>): Promise<ApiResult<AppSettings>> {
  return serve(
    COMMANDS.settingsUpdate,
    async () => {
      await latency();
      const next = commitDb((db) => ({
        ...db,
        settings: {
          ...db.settings,
          ...patch,
          appearance: { ...db.settings.appearance, ...patch.appearance },
          reminder: { ...db.settings.reminder, ...patch.reminder },
        },
      }));
      return next.settings;
    },
    { args: { patch } },
  );
}

/**
 * 同步状态：云同步尚未实现，这里如实返回"仅本机"（lastSyncedAt 为 null），
 * 不再显示一个假的"已同步"。
 */
export function getSyncStatus(options: RequestOptions = {}): Promise<ApiResult<SyncStatus>> {
  return serve(
    COMMANDS.syncStatusGet,
    async () => {
      await latency(options.signal);
      const db = getDb();
      return {
        state: "idle",
        lastSyncedAt: null,
        pendingChanges: 0,
        deviceName: "本机",
        storageUsedKb: localDataSummaryOf(db).sizeKb,
      };
    },
    { request: options },
  );
}

/** 云同步属于后续阶段，明确报"未实现"，避免误导 */
export function triggerSync(): Promise<ApiResult<SyncStatus>> {
  return serve(
    COMMANDS.syncTrigger,
    async () => {
      await latency();
      throw new Error("not_implemented:云同步将在后续版本提供，当前数据仅保存在本机。");
    },
    { args: {} },
  );
}

export function getLocalData(
  options: RequestOptions = {},
): Promise<ApiResult<LocalDataSummary>> {
  return serve(
    COMMANDS.localDataSummary,
    async () => {
      await latency(options.signal);
      return localDataSummaryOf(getDb());
    },
    { request: options },
  );
}

/** 真正清空学习记录（保留词库内容与个人资料），并重排计划 */
export function clearLocalData(): Promise<ApiResult<{ cleared: true }>> {
  return serve(
    COMMANDS.localDataClear,
    async () => {
      await latency();
      const now = new Date();
      commitDb((db) => rebuildPlan(clearLearningRecords(db, now), { reason: "regenerate", now }));
      return { cleared: true } as const;
    },
    { args: {} },
  );
}
