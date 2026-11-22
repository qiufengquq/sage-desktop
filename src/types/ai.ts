/**
 * SAGE Intelligence 集成契约的 TypeScript 侧。
 *
 * 与 `src-tauri/src/models/ai.rs` 一一对应（camelCase）。
 * 这里不出现任何"框架内部结构"：只有版本、状态、Provider 视图与输出结果。
 * 契约说明见 docs/AI_INTEGRATION.md。
 */

/** 运行时状态：未初始化 / 初始化中 / 正常 / 异常 / 已关闭 */
export type AiRuntimeStatus = "uninitialized" | "initializing" | "ready" | "error" | "shutdown";

export type AiProtocol = "openai-compatible" | "anthropic";

export interface AiVersionInfo {
  clientVersion: string;
  intelligenceVersion: string;
  intelligenceCommit: string;
  /** 客户端 ⇄ 框架的集成契约版本，与两个项目的 SemVer 无关 */
  protocolVersion: string;
  /** 恒为 "library"：AI 与客户端同进程内嵌，不存在 sidecar 模式 */
  integrationMode: string;
  buildTime: string;
  platform: string;
}

export interface AiProviderView {
  id: string;
  displayName: string;
  protocol: AiProtocol | string;
  baseUrl: string;
  enabled: boolean;
  defaultModel: string | null;
  models: string[];
  /** 只告诉前端"有没有密钥"，密钥本身永不出后端 */
  hasApiKey: boolean;
  updatedAt: string;
}

export interface AiStatus {
  status: AiRuntimeStatus;
  startedAt: string | null;
  lastError: string | null;
  providers: AiProviderView[];
  defaultProvider: string | null;
  defaultModel: string | null;
  activeStreams: number;
  version: AiVersionInfo;
}

export interface AiProviderInput {
  id: string;
  displayName: string;
  protocol: AiProtocol | string;
  baseUrl: string;
  defaultModel?: string | null;
  models?: string[];
  enabled?: boolean;
  /** 缺省 = 不修改已有密钥；空串 = 删除密钥 */
  apiKey?: string | null;
}

export interface AiProbeResult {
  providerId: string;
  model: string;
  ok: boolean;
  latencyMs: number;
  message: string | null;
}

export interface AiError {
  code: string;
  message: string;
  retryable: boolean;
}

export interface AiCompletionResult {
  text: string;
  reasoning: string;
  interrupted: boolean;
  error: AiError | null;
  /** 上游 usage 原样透传，字段随 Provider 不同 */
  usage: unknown;
  finishReason: unknown;
  model: string | null;
}

export interface AiStreamHandle {
  requestId: string;
  providerId: string;
  model: string;
}

/** `sage-ai://stream-finished` 的载荷 */
export interface AiStreamFinished {
  requestId: string;
  text: string;
  reasoning: string;
  interrupted: boolean;
  finished: boolean;
  usage?: unknown;
  finishReason?: unknown;
  error?: AiError | null;
}

/**
 * 框架事件名（含 `ai-core://` 前缀）。
 * 与 `ai-core-tauri/src/event.rs::event_name` 的返回值保持一致，不允许重命名。
 */
export const AI_EVENTS = {
  started: "ai-core://started",
  textDelta: "ai-core://text-delta",
  reasoningDelta: "ai-core://reasoning-delta",
  toolCallStarted: "ai-core://tool-call-started",
  toolCallDelta: "ai-core://tool-call-delta",
  toolCallCompleted: "ai-core://tool-call-completed",
  toolCallExecuted: "ai-core://tool-call-executed",
  structuredOutput: "ai-core://structured-output",
  usageUpdated: "ai-core://usage-updated",
  error: "ai-core://error",
  completed: "ai-core://completed",
  /** 客户端自定义：一次流式请求的终态摘要 */
  finished: "sage-ai://stream-finished",
} as const;

/** 一条流式事件（已带顶层 type 便于后端改写，字段原样保留） */
export interface AiStreamEvent {
  event: string;
  payload: Record<string, unknown>;
}
