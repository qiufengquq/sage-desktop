import { addDays, isoDateTime } from "@/lib/date";

/** 可复现的伪随机数：保证每次启动看到的是同一份"真实"数据 */
export function createRng(seed: number) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function pick<T>(items: T[], r: () => number): T {
  return items[Math.floor(r() * items.length)] as T;
}

export function range(n: number) {
  return Array.from({ length: n }, (_, i) => i);
}

/** 相对今天的时间戳，便于模拟数据始终"新鲜" */
export function daysAgo(days: number, hour = 20, minute = 0) {
  const d = addDays(new Date(), -days);
  d.setHours(hour, minute, 0, 0);
  return isoDateTime(d);
}

export function daysAhead(days: number, hour = 9, minute = 0) {
  return daysAgo(-days, hour, minute);
}

const MOCK_LATENCY_RANGE: [number, number] = [
  Number(import.meta.env.VITE_SAGE_MOCK_LATENCY ?? 90),
  320,
];

/**
 * 模拟网络往返。真实后端接入后由 transport 层直接忽略。
 * 使用 AbortSignal 保证页面切换时不会更新已卸载组件。
 */
export function latency(signal?: AbortSignal): Promise<void> {
  const [min, max] = MOCK_LATENCY_RANGE;
  const ms = min + Math.random() * Math.max(0, max - min);
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new DOMException("Aborted", "AbortError"));
      return;
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    function onAbort() {
      clearTimeout(timer);
      reject(new DOMException("Aborted", "AbortError"));
    }
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}

/** Mock 层统一错误：交由 transport 转成 ApiError */
export class MockError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "MockError";
    this.code = code;
  }
}
