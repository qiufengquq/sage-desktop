import type { LearningDb } from "@/types";
import { SCHEMA_VERSION } from "./seed";

/**
 * 持久化仓储接口。
 *
 * 这一层是未来 Rust + SQLite 的镜像：此时只有 load / save / clear 三个方法，
 * 第二阶段换成 SQLite 时，只需实现同样的接口，上层 service 一行都不用改。
 */
export interface LearningRepository {
  load(): LearningDb | null;
  save(db: LearningDb): void;
  clear(): void;
}

export const STORAGE_KEY = "sage.learning.db";

function safeStorage(): Storage | null {
  try {
    if (typeof window === "undefined" || !window.localStorage) return null;
    const probe = "__sage_probe__";
    window.localStorage.setItem(probe, "1");
    window.localStorage.removeItem(probe);
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * 最小校验：拒绝结构不完整的数据，宁可重新种子化，也不要把坏数据渲染成 UI。
 * schemaVersion 不匹配时返回 null（第一阶段只有 v1，尚未引入迁移）。
 */
export function parseDb(raw: unknown): LearningDb | null {
  if (typeof raw !== "object" || raw === null) return null;
  const candidate = raw as Partial<LearningDb>;

  if (candidate.schemaVersion !== SCHEMA_VERSION) return null;
  if (!candidate.profile || typeof candidate.profile !== "object") return null;
  if (!candidate.entityStates || typeof candidate.entityStates !== "object") return null;
  if (!Array.isArray(candidate.trainingAttempts)) return null;
  if (!Array.isArray(candidate.assessmentAttempts)) return null;
  if (!Array.isArray(candidate.reviewLogs)) return null;
  if (!Array.isArray(candidate.masteryEvents)) return null;
  if (!candidate.settings) return null;

  return candidate as LearningDb;
}

export function createLocalRepository(storage: Storage | null = safeStorage()): LearningRepository {
  return {
    load() {
      if (!storage) return null;
      try {
        const raw = storage.getItem(STORAGE_KEY);
        if (!raw) return null;
        return parseDb(JSON.parse(raw));
      } catch (error) {
        console.warn("[sage] 本地数据读取失败，将重新初始化", error);
        return null;
      }
    },
    save(db) {
      if (!storage) return;
      try {
        storage.setItem(STORAGE_KEY, JSON.stringify(db));
      } catch (error) {
        console.warn("[sage] 本地数据写入失败", error);
      }
    },
    clear() {
      if (!storage) return;
      try {
        storage.removeItem(STORAGE_KEY);
      } catch (error) {
        console.warn("[sage] 本地数据清理失败", error);
      }
    },
  };
}

/** 测试与浏览器预览共用同一套语义的内存实现 */
export function createMemoryRepository(initial: LearningDb | null = null): LearningRepository {
  let current = initial;
  return {
    load: () => current,
    save: (db) => {
      current = db;
    },
    clear: () => {
      current = null;
    },
  };
}
