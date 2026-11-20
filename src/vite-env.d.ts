/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** mock = 使用本地模拟数据；backend = 调用 Tauri Commands */
  readonly VITE_SAGE_DATA_SOURCE?: "mock" | "backend";
  /** 模拟数据延迟下限（毫秒） */
  readonly VITE_SAGE_MOCK_LATENCY?: string;
  readonly VITE_SAGE_AI_MODEL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

interface Window {
  __TAURI_INTERNALS__?: unknown;
}
