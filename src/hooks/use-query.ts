import { useCallback, useEffect, useRef, useState } from "react";
import type { ApiError, ApiResult, RequestOptions } from "@/types";
import { useDataVersion } from "@/stores/learning-store";

export interface QueryState<T> {
  data: T | null;
  error: ApiError | null;
  loading: boolean;
  /** 首次加载完成前为 true，用于区分骨架屏与刷新态 */
  initial: boolean;
  reload: () => void;
}

/**
 * 统一的数据获取 hook。
 * - 组件卸载或依赖变化时通过 AbortSignal 取消请求
 * - 页面只面对 ApiResult，不关心 Mock / Tauri 的差异
 */
export function useQuery<T>(
  fetcher: (options: RequestOptions) => Promise<ApiResult<T>>,
  deps: unknown[] = [],
): QueryState<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [loading, setLoading] = useState(true);
  const [initial, setInitial] = useState(true);
  const [tick, setTick] = useState(0);

  // 用 ref 持有最新 fetcher：调用点通常会内联箭头函数，直接放进依赖会无限请求。
  // 写入必须发生在 effect 中，渲染期写 ref 会让 React Compiler 放弃优化。
  const fetcherRef = useRef(fetcher);
  useEffect(() => {
    fetcherRef.current = fetcher;
  });

  const depsKey = JSON.stringify(deps);
  // 任何领域写入都会递增 version，挂载中的查询随之刷新，
  // 这就是"训练完成后首页/知识库立即变化"的实现方式。
  const version = useDataVersion();

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(null);

    fetcherRef
      .current({ signal: controller.signal })
      .then((result) => {
        if (controller.signal.aborted) return;
        if (result.ok) {
          setData(result.data);
          setError(null);
        } else {
          setError(result.error);
        }
      })
      .catch((err: unknown) => {
        if (controller.signal.aborted) return;
        if (err instanceof DOMException && err.name === "AbortError") return;
        setError({
          code: "internal",
          message: "读取数据时出错，请重试。",
          detail: err instanceof Error ? err.message : String(err),
          retryable: true,
        });
      })
      .finally(() => {
        if (controller.signal.aborted) return;
        setLoading(false);
        setInitial(false);
      });

    return () => controller.abort();
    // depsKey 承载依赖的序列化结果，避免数组字面量导致的无限请求
  }, [depsKey, tick, version]);

  const reload = useCallback(() => setTick((t) => t + 1), []);

  return { data, error, loading, initial, reload };
}
