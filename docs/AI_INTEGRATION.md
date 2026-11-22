# SAGE Intelligence 集成契约

本文件描述 **SAGE Client（React + Tauri）** 与 **SAGE Intelligence（Rust 库）** 之间的稳定接口。
两个项目版本独立演进，契约变更必须在此登记并递增 `AI Integration Protocol` 版本。

---

## 1. 版本

| 项 | 值 | 来源 |
|---|---|---|
| SAGE Client | `1.0.1` | `package.json` / `src-tauri/Cargo.toml` / `tauri.conf.json` |
| SAGE Intelligence | `0.1.0` | `Cargo.lock` → `cargo:rustc-env` |
| Intelligence Commit | `33be312c6949afa7f20051ec40fd520002ec5da6` | 同上（`source = "git+...#<sha>"`） |
| AI Integration Protocol | `1` | `src-tauri/src/ai/version.rs::PROTOCOL_VERSION` |
| 集成方式 | `library`（唯一模式） | 与客户端编译进同一可执行文件，同一 Rust 进程 |
| Build Channel | `stable` | — |

> 上游目前 **0 tag / 0 release**，因此 `Cargo.toml` 使用 `rev = "<commit>"` 锁定。
> 版本与 commit 由 `src-tauri/build.rs` 解析 `Cargo.lock` 注入，**不允许**在代码里手写。

依赖声明（`src-tauri/Cargo.toml`）：

```toml
ai-core = { git = "https://github.com/qiufengquq/sage-intelligence", rev = "33be312c..." }
ai-core-tauri = { git = "https://github.com/qiufengquq/sage-intelligence", rev = "33be312c..." }
```

---

## 2. 命令契约

所有命令返回既有 `ApiResult<T>`：成功 `{ ok: true, data }`，失败 `{ ok: false, error: { code, message, detail, retryable } }`。

### 2.1 Runtime

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `ai_status` | — | `AiStatus` | 已实现 |
| `ai_version` | — | `AiVersionInfo` | 已实现 |

`AiStatus`：

```ts
{
  status: "uninitialized" | "initializing" | "ready" | "error" | "shutdown";
  startedAt: string | null;
  lastError: string | null;
  providers: AiProviderView[];
  defaultProvider: string | null;
  defaultModel: string | null;
  activeStreams: number;
  version: AiVersionInfo;
}
```

### 2.2 Provider

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `ai_provider_upsert` | `{ provider: AiProviderInput }` | `AiProviderView` | 已实现 |
| `ai_provider_delete` | `{ providerId }` | `boolean` | 已实现 |
| `ai_provider_test` | `{ providerId? }` | `AiProbeResult` | 已实现（用一次极小真实请求探测，不是上游专用 API） |

`AiProviderInput`：

```ts
{
  id: string;
  displayName?: string;
  protocol?: "openai-compatible" | "anthropic";
  baseUrl: string;
  defaultModel?: string | null;
  models?: string[];
  enabled?: boolean;
  apiKey?: string | null;   // 缺省 = 不修改；"" = 删除
}
```

`AiProviderView` 只包含 `hasApiKey: boolean`，**不含密钥**。

### 2.3 请求与流式

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `ai_complete` | `{ prompt, providerId?, model?, system? }` | `AiCompletionResult` | 已实现 |
| `ai_stream_start` | 同上 | `AiStreamHandle` | 已实现 |
| `ai_stream_cancel` | `{ requestId }` | `boolean` | 已实现 |

- `providerId` / `model` 都缺省时，路由到第一个启用的 Provider 的默认模型；完全未配置时返回 `unavailable`。
- 取消是协作式的：`ai_stream_cancel` 触发 `CallContext` 取消令牌，流在下一个 poll 停止，终态带 `interrupted: true` 与部分文本。
- 一次性结果里 `usage` / `finishReason` 是上游 JSON 原样透传（不同 Provider 字段不同），客户端不做字段建模。

### 2.4 Agent / Workflow / Conversation / Memory

| 能力 | 状态 | 说明 |
|---|---|---|
| Agent 执行 / 状态 / 取消 | **planned** | 框架没有 Agent 抽象，只有 `OutputEngine` 单次/流式调用与工具轮次 |
| Workflow 执行 / 结果 | **planned** | 框架只有 `RoundLoop`（工具轮次），没有工作流编排 API |
| Conversation 创建 / 读取 | **planned** | 框架 `memory` 模块提供 `MemoryStore`，客户端尚未接入 |
| Memory 查询 / 更新 / 清理 | **planned** | 同上；AI 记忆目录已预留（`ai-core-data/ai-memory/`） |
| 工具调用 | **未启用** | 客户端不注册任何工具，因此模型无法触发工具；这是安全边界而非缺陷 |
| 用量看板 | **planned** | 框架 `usage` 会写 `ai-core-data/ai-metadata/usage.sqlite3`，UI 未接入 |

> 原则：未实现的能力不提供假接口、不返回伪造数据。前端遇到 `planned` 能力不会有入口。

---

## 3. 事件契约

事件由 `ai-core-tauri::EventEmitter` 产生，宿主通过 `AppHandle::emit` 转发。
事件名带 `ai-core://` 前缀，**不允许重命名**。

| 事件 | 载荷要点 |
|---|---|
| `ai-core://started` | 请求开始 |
| `ai-core://text-delta` | `{ type, text }` 增量文本 |
| `ai-core://reasoning-delta` | `{ type, text }` 推理增量 |
| `ai-core://tool-call-started` | 工具调用开始（客户端未启用工具） |
| `ai-core://tool-call-delta` | 工具参数增量 |
| `ai-core://tool-call-completed` | 工具调用完成 |
| `ai-core://tool-call-executed` | 工具执行结果 |
| `ai-core://structured-output` | 结构化输出通过校验 |
| `ai-core://usage-updated` | 用量更新 |
| `ai-core://error` | `{ code, message, retryable }`（已脱敏） |
| `ai-core://completed` | 流结束（含 `interrupted`） |
| `sage-ai://stream-finished` | **客户端自定义**：终态摘要 `{ requestId, text, reasoning, interrupted, finished, usage, finishReason, error }` |

前端统一从 `aiService.subscribe()` 订阅，页面不直接 `listen`。

---

## 4. 数据归属

| 数据 | 位置 | 归属 |
|---|---|---|
| Provider 配置（非密钥） | 业务库 `sage.db` → `ai_providers` 表（迁移 `0003_ai.sql`） | 客户端 |
| API Key | `app_data_dir/ai/provider-keys.json` | 客户端（仅 Rust 读写） |
| AI memory / metadata / cache | `app_data_dir/ai-core-data/`（框架自管） | SAGE Intelligence |

约束：

1. **不共享连接**：AI 模块永远是"借用"业务库连接读配置，不新建第二个 `Database`。
2. **不共享文件**：框架存储根目录在 `ai-core-data/`，与 `sage.db` 物理分离；`local_data_clear` 不影响 AI 目录。
3. **不破坏迁移**：`0003_ai` 只新增表，不改既有表；`SCHEMA_VERSION` 2 → 3。
4. **密钥不落业务库、不进备份导出、不回传前端**。

---

## 5. 错误映射

框架 `CoreError` → 客户端既有错误码（不新增前端不认识的 code）：

| ErrorKind | ApiErrorCode |
|---|---|
| `Authentication` | `unauthorized` |
| `InvalidArgument` / `Config` / `CapabilityUnsupported` / `TokenLimitExceeded` / `Serialization` / `Tool` | `invalid_argument` |
| `Timeout` / `Cancelled` | `timeout` |
| `NotFound` | `not_found` |
| 其它（`Provider` / `Io` / `Concurrency` …） | `unavailable` |

`message` 由框架脱敏（`[REDACTED]`），不会带出密钥或 prompt 原文。

---

## 6. 安全边界

1. API Key 只存在于 Rust 侧内存与独立文件；`AiProviderView` 只有 `hasApiKey`。
2. 未注册任何 `ToolRegistry`，AI 无法执行命令、读写任意文件。
3. 客户端不暴露"打开本地文件/执行命令"的 AI 命令。
4. 事件与错误信息均使用框架脱敏后的内容。
5. 已知限制：未接入 Windows Credential Manager，密钥以文件形式存放（见 RELEASE_NOTES）。

---

## 7. 升级流程

1. 修改 `src-tauri/Cargo.toml` 中**两处** `rev`（`ai-core` 与 `ai-core-tauri` 必须一致）。
2. `cargo check` 确认可编译。
3. `build.rs` 会自动把新的版本与 commit 注入，应用内「设置 → AI 集成」随即可见。
4. 若事件名/命令参数/错误码映射有变，递增 `PROTOCOL_VERSION` 并更新本文件。
5. 在 `docs/RELEASE_NOTES_<version>.md` 记录旧/新 commit。

---

## 8. 明确不采用的架构

以下方案已正式排除，**不做预埋、不留开关**：

| 排除项 | 说明 |
|---|---|
| Sidecar | 不启动任何伴随进程 |
| 独立 Runtime 进程 / 独立 EXE | AI 代码只存在于客户端可执行文件内 |
| IPC / 本地 HTTP 服务 | 前后端只通过 Tauri Command 与 Event 通信 |
| 独立安装包 / 下载器 / 更新系统 | 不存在"AI 框架升级"这一独立动作 |
| 运行时版本切换 / 回滚 / 完整性校验 | 版本由构建决定，不可在运行时更换 |

`integration_mode` 恒为 `library`：AI 与客户端编译进同一个可执行文件，运行在同一个
Rust 进程内。框架升级 = 修改 `Cargo.toml` 的 `rev` + 重新构建安装包。

同时明确：以下内容**不在**安装包内，也不由本集成提供 —— 模型权重、外部 Provider API、
用户自填的 API Key 与配置数据。安装包只包含编译后的框架代码。
