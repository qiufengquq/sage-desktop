/**
 * 统一 API 传输层契约。
 * React 不直接依赖 Mock 实现，也不直接依赖 Tauri（后者只在 transport 层出现）。
 */

export type ApiErrorCode =
  | "unavailable"
  | "not_found"
  | "invalid_argument"
  | "unauthorized"
  | "rate_limited"
  | "timeout"
  | "internal"
  /** 本地数据库版本高于当前程序：必须升级应用，重试无用 */
  | "database_too_new";

export interface ApiError {
  code: ApiErrorCode;
  /** 面向用户的说明：出了什么问题 */
  message: string;
  /** 面向用户的建议：怎么修复 */
  hint?: string;
  /** 便于排查的技术细节，不直接展示给用户 */
  detail?: string;
  retryable: boolean;
}

export type ApiResult<T> =
  | { ok: true; data: T }
  | { ok: false; error: ApiError };

export interface RequestOptions {
  signal?: AbortSignal;
  timeoutMs?: number;
}

/** 分页信封，未来 Rust 后端返回列表时统一使用 */
export interface Page<T> {
  items: T[];
  total: number;
  cursor?: string;
}

export const ok = <T>(data: T): ApiResult<T> => ({ ok: true, data });

export const fail = (
  code: ApiErrorCode,
  message: string,
  extra?: Partial<Omit<ApiError, "code" | "message">>,
): ApiResult<never> => ({
  ok: false,
  error: { code, message, retryable: false, ...extra },
});
