import type { ApiError, ApiErrorCode, ApiResult, RequestOptions } from "@/types";
import { ok } from "@/types/api";
import { COMMAND_STATUS, type CommandName } from "./contracts";

export type DataSource = "mock" | "backend";

/**
 * 数据源开关：
 * - mock：全部使用本地模拟数据（第一阶段默认）
 * - backend：通过 Tauri Command 调用 Rust 后端
 * 通过 .env 中的 VITE_SAGE_DATA_SOURCE 切换，页面代码无需改动。
 */
export const dataSource: DataSource = import.meta.env.VITE_SAGE_DATA_SOURCE ?? "mock";

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const ERROR_CODE_MAP: Record<string, ApiErrorCode> = {
  not_found: "not_found",
  invalid_argument: "invalid_argument",
  unauthorized: "unauthorized",
  rate_limited: "rate_limited",
  timeout: "timeout",
  internal: "internal",
  unavailable: "unavailable",
  database_too_new: "database_too_new",
};

/** 把 Rust 侧的错误字符串 / 结构统一成 ApiError */
export function normalizeError(error: unknown): ApiError {
  if (isApiError(error)) return error;

  if (error instanceof Error && error.name === "AbortError") {
    return { code: "timeout", message: "请求已取消", retryable: false };
  }

  if (typeof error === "string") {
    const [rawCode, ...rest] = error.split(":");
    const code = ERROR_CODE_MAP[rawCode.trim()] ?? "internal";
    const message = rest.join(":").trim() || rawCode.trim();
    return {
      code,
      message: toUserMessage(code),
      detail: message,
      retryable: code === "unavailable" || code === "timeout",
    };
  }

  if (error instanceof Error) {
    return {
      code: "internal",
      message: "操作没有完成，请稍后重试。",
      detail: error.message,
      retryable: true,
    };
  }

  return {
    code: "internal",
    message: "操作没有完成，请稍后重试。",
    detail: String(error),
    retryable: true,
  };
}

function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value &&
    "retryable" in value
  );
}

function toUserMessage(code: ApiErrorCode): string {
  switch (code) {
    case "not_found":
      return "没有找到这条数据，它可能已被删除。";
    case "invalid_argument":
      return "输入的内容不符合要求，请检查后重试。";
    case "unauthorized":
      return "当前操作需要登录后才能进行。";
    case "rate_limited":
      return "请求过于频繁，请稍等片刻再试。";
    case "timeout":
      return "请求超时，请检查网络后重试。";
    case "unavailable":
      return "本地服务暂时不可用，请重新打开应用。";
    case "database_too_new":
      return "本地数据由更高版本的 Sage 创建，请先升级应用再打开。";
    default:
      return "操作没有完成，请稍后重试。";
  }
}

async function invokeBackend<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

interface ServeOptions {
  args?: Record<string, unknown>;
  request?: RequestOptions;
}

/**
 * 该命令是否已有 Rust 实现。
 * 未登记或标记为 `planned` 的命令一律走 mock，避免"打包后整页报不可用"。
 */
export function isBackendReady(command: string): boolean {
  const status = COMMAND_STATUS[command as CommandName];
  return status === "ready";
}

/**
 * 统一的服务调用入口。
 * 页面与 hooks 只看到 Promise<ApiResult<T>>，不关心背后是 Mock 还是 Rust。
 */
export async function serve<T>(
  command: string,
  mock: () => Promise<T>,
  options: ServeOptions = {},
): Promise<ApiResult<T>> {
  const useMock = dataSource === "mock" || !isTauriRuntime() || !isBackendReady(command);

  if (useMock) {
    try {
      const data = await mock();
      return ok(data);
    } catch (error) {
      // 取消请求不当作错误返回，交给调用方忽略
      if (error instanceof DOMException && error.name === "AbortError") throw error;
      return { ok: false, error: normalizeError(error) };
    }
  }

  try {
    const data = await invokeBackend<T>(command, options.args);
    return ok(data);
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") throw error;
    return { ok: false, error: normalizeError(error) };
  }
}
