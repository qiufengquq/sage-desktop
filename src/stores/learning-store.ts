import { create } from "zustand";
import type { LearningDb } from "@/types";
import { buildSeedDb } from "@/services/data/seed";
import {
  createLocalRepository,
  type LearningRepository,
} from "@/services/data/repository";

/**
 * 领域状态仓库。
 *
 * 约定：
 * - 页面不直接调用这里的写方法，只能通过 services/api 间接使用（保持单一入口）。
 * - `revision` 每次提交递增，`useQuery` 依赖它来在所有相关页面同步刷新。
 * - 所有写入都是"整份快照替换"，对应未来 SQLite 的一个事务。
 */

interface LearningStoreState {
  db: LearningDb;
  revision: number;
  hydrated: boolean;
  /** 首次加载：读仓储，失败则种子化 */
  init: (now?: Date) => void;
  /** 事务式提交：mutator 必须返回新的 db 对象 */
  commit: (mutator: (db: LearningDb) => LearningDb) => void;
  /** 整体替换（清理数据 / 测试注入） */
  replace: (db: LearningDb) => void;
}

let repository: LearningRepository = createLocalRepository();

/** 测试或 Tauri 侧替换存储实现时使用 */
export function setLearningRepository(next: LearningRepository): void {
  repository = next;
}

export function getLearningRepository(): LearningRepository {
  return repository;
}

function createInitialDb(): LearningDb {
  return buildSeedDb(new Date());
}

export const useLearningStore = create<LearningStoreState>((set, get) => ({
  db: createInitialDb(),
  revision: 0,
  hydrated: false,

  init: (now = new Date()) => {
    if (get().hydrated) return;
    const loaded = repository.load();
    const db = loaded ?? buildSeedDb(now);
    if (!loaded) repository.save(db);
    set((state) => ({ db, hydrated: true, revision: state.revision + 1 }));
  },

  commit: (mutator) => {
    const current = get();
    const next = mutator(current.db);
    if (next === current.db) return;
    repository.save(next);
    set({ db: next, revision: current.revision + 1 });
  },

  replace: (db) => {
    repository.save(db);
    set((state) => ({ db, hydrated: true, revision: state.revision + 1 }));
  },
}));

/** 供 service 使用的同步读取入口（保证已初始化） */
export function getDb(now: Date = new Date()): LearningDb {
  const store = useLearningStore.getState();
  if (!store.hydrated) store.init(now);
  return useLearningStore.getState().db;
}

/** 供 service 使用的提交入口 */
export function commitDb(mutator: (db: LearningDb) => LearningDb): LearningDb {
  const store = useLearningStore.getState();
  if (!store.hydrated) store.init();
  useLearningStore.getState().commit(mutator);
  return useLearningStore.getState().db;
}

/** 页面通过 useQuery 依赖这个版本号实现跨页同步 */
export function useDataVersion(): number {
  return useLearningStore((state) => state.revision);
}

/** 测试用：重置为种子数据 */
export function resetLearningStore(now: Date = new Date()): LearningDb {
  const db = buildSeedDb(now);
  useLearningStore.getState().replace(db);
  return db;
}
