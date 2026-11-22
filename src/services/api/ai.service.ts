/**
 * SAGE Intelligence 的唯一前端入口。
 *
 * 约束：
 *   - 页面/组件只能通过这里访问 AI，禁止直接 import `@tauri-apps/api` 的 invoke；
 *   - 非 Tauri 运行时（浏览器 / mock）明确返回 `unavailable`，**不伪造 AI 结果**；
 *   - 流式过程走事件，不塞进一次 `ApiResult`（见 docs/AI_INTEGRATION.md）。
 */
import type { ApiResult } from "@/types";
import type {
  AiCompletionResult,
  AiProbeResult,
  AiProviderInput,
  AiProviderView,
  AiStatus,
  AiStreamEvent,
  AiStreamFinished,
  AiStreamHandle,
  AiVersionInfo,
} from "@/types/ai";
import { AI_EVENTS } from "@/types/ai";
import { COMMANDS } from "./contracts";
import { isTauriRuntime, serve } from "./transport";

/** 只有桌面端 Rust 后端能提供 AI 能力 */
const NO_BACKEND = "unavailable:AI 能力只在桌面端（Tauri + Rust）可用";

function denyInMock(): never {
  throw NO_BACKEND;
}

export function getStatus(): Promise<ApiResult<AiStatus>> {
  return serve(COMMANDS.aiStatus, async () => denyInMock());
}

export function getVersion(): Promise<ApiResult<AiVersionInfo>> {
  return serve(COMMANDS.aiVersion, async () => denyInMock());
}

export function upsertProvider(input: AiProviderInput): Promise<ApiResult<AiProviderView>> {
  return serve(COMMANDS.aiProviderUpsert, async () => denyInMock(), {
    args: { provider: input },
  });
}

export function deleteProvider(providerId: string): Promise<ApiResult<boolean>> {
  return serve(COMMANDS.aiProviderDelete, async () => denyInMock(), {
    args: { providerId },
  });
}

/** 连接探测：发一条极小请求验证密钥/模型/网络链路 */
export function testProvider(providerId?: string): Promise<ApiResult<AiProbeResult>> {
  return serve(COMMANDS.aiProviderTest, async () => denyInMock(), {
    args: { providerId },
  });
}

export interface AiPromptInput {
  prompt: string;
  providerId?: string;
  model?: string;
  system?: string;
}

export function complete(input: AiPromptInput): Promise<ApiResult<AiCompletionResult>> {
  return serve(COMMANDS.aiComplete, async () => denyInMock(), { args: { ...input } });
}

export function startStream(input: AiPromptInput): Promise<ApiResult<AiStreamHandle>> {
  return serve(COMMANDS.aiStreamStart, async () => denyInMock(), { args: { ...input } });
}

export function cancelStream(requestId: string): Promise<ApiResult<boolean>> {
  return serve(COMMANDS.aiStreamCancel, async () => denyInMock(), {
    args: { requestId },
  });
}

export interface AiEventHandlers {
  /** 框架事件：started / text-delta / usage-updated / error ... */
  onEvent?: (event: AiStreamEvent) => void;
  /** 客户端终态事件：sage-ai://stream-finished */
  onFinished?: (payload: AiStreamFinished) => void;
}

/**
 * 订阅 AI 事件。返回退订函数。
 * 非桌面环境直接返回 noop —— 调用方不需要分支。
 */
export async function subscribe(handlers: AiEventHandlers): Promise<() => void> {
  if (!isTauriRuntime()) return () => {};

  const { listen } = await import("@tauri-apps/api/event");
  const unlisteners = await Promise.all(
    Object.values(AI_EVENTS).map((name) =>
      listen<Record<string, unknown>>(name, (event) => {
        if (name === AI_EVENTS.finished) {
          handlers.onFinished?.(event.payload as unknown as AiStreamFinished);
          return;
        }
        handlers.onEvent?.({ event: name, payload: event.payload });
      }),
    ),
  );

  return () => {
    for (const unlisten of unlisteners) unlisten();
  };
}

/** 文本增量提取（框架 payload: { type, text }） */
export function textDeltaOf(event: AiStreamEvent): string {
  if (event.event !== AI_EVENTS.textDelta) return "";
  const text = event.payload.text;
  return typeof text === "string" ? text : "";
}
