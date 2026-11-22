# SAGE Client 1.0.1 — SAGE Intelligence 集成审查

- 审查对象：当前仓库 `sage-desktop` 实际代码（非文档、非计划）
- 审查时间：2026-11-22
- 审查结论：**可以集成，但存在 3 个必须先解决的阻塞项**（见 §8）
- 本文档只记录审查结果，不代表任何功能已完成

---

## 1. 当前客户端架构（实测）

### 1.1 技术栈

| 层 | 实际使用 |
|---|---|
| 前端 | React 19 + TypeScript 7 + Vite 8 + Tailwind CSS 4 + Radix/shadcn 风格组件 + Zustand 5 + sonner |
| 桌面壳 | Tauri 2（`tauri = "2"`，capabilities 仅 `core:default`） |
| 后端 | Rust 2021，`rust-version = "1.77"` |
| 数据库 | SQLite（`rusqlite 0.32` bundled），WAL，单写连接 + `Mutex` 串行化 |

Rust 依赖只有 5 个：`tauri`、`serde`、`serde_json`、`rusqlite`、`chrono`、`sha2`。
**当前没有任何 HTTP 客户端、没有 tokio 直接依赖、没有任何 AI SDK。**

### 1.2 目录职责（实际）

```
src/
  app/            路由与全局 store（ui-store）
  components/     设计系统与通用组件（41 个文件）
  features/       七个业务模块页面（dashboard / review / training / assessment /
                  knowledge / planner / analytics / settings）
  hooks/          use-query 等
  lib/            纯函数算法（srs / mastery / xp / grading / stats / planner / ability）
  services/
    api/          唯一数据入口：contracts.ts + transport.ts + *.service.ts
    data/         本地派生层（selectors / learning-db），mock 模式使用
    mock/         静态内容包（词/语法/句/题目）
  stores/         learning-store（mock 数据库状态）
  types/          与 Rust DTO 一一对应的 TypeScript 类型

src-tauri/src/
  algorithms/     纯算法（srs / mastery / xp / grading / selection / layout / ability…）
  commands/       app / user / learning / knowledge
  database/       连接、迁移、备份、内容同步
  models/         DTO（camelCase 序列化）
  repositories/   纯 SQL，只吃 &Connection
  services/       事务编排（training / assessment / review / knowledge / answer / state_updater）
  state.rs        AppState { db: Arc<Database> }
```

### 1.3 数据源切换（不可破坏）

`src/services/api/transport.ts` 是唯一出口：

```
serve(command, mockFn, { args })
  ├─ mock      : VITE_SAGE_DATA_SOURCE=mock 或非 Tauri 或命令未 ready
  └─ backend   : invoke(command, args)
```

`contracts.ts` 的 `COMMAND_STATUS` 按命令声明 `ready | planned`：
`planned` 的命令在打包后自动回退 mock，避免整页不可用。
**新增 AI 命令必须同时进 `COMMANDS` 与 `COMMAND_STATUS`，否则前端不会调用后端。**

---

## 2. 当前已实现功能（实测）

已接 SQLite（`ready`）：
`user_profile_get/update`、`settings_get/update`、`sync_status_get`、`local_data_summary`、
`assessment_modules/submit/check`、`knowledge_graph/list/detail/neighbors`、
`training_options/start/submit/check`、`review_queue/review_submit`

仍为占位（Rust 返回 `unavailable`，前端回退 mock）：
`dashboard_snapshot`、`analytics_query`、`sync_trigger`、`local_data_clear`、
`plan_get`、`plan_regenerate`、`plan_apply_difficulty`、`plan_toggle_item`

> 关键事实：**AI 规划功能目前只存在于前端 mock，Rust 侧没有任何 AI 代码。**
> `plan_*` 四个命令都是 `AppError::unavailable("AI …")`。

---

## 3. 当前后端能力

### 3.1 AppState

```rust
pub struct AppState { pub db: Arc<Database> }
```

初始化顺序（`state.rs::init`）：定位路径 → 打开库（PRAGMA → 迁移 → 内容包同步）→ 首启种子。
数据目录可用 `SAGE_DB_PATH` 覆盖（本次验收即用此机制隔离真实数据）。

### 3.2 数据库

- `SCHEMA_VERSION = 2`，迁移脚本：`0001_init.sql`、`0002_model_alignment.sql`
- 36 张业务表 + 1 个视图（`knowledge_index`）
- **`database/mod.rs` 的单元测试硬断言 `tables == 36`**，新增表必须同步修改该断言
- 业务库位置：`app_data_dir()/sage.db`

### 3.3 事务与错误

- 写操作统一 `Database::with_tx`（`BEGIN IMMEDIATE` + 失败回滚 + `db_revision` 递增）
- 错误统一 `AppError`，序列化为 `"code:message"`，前端 `normalizeError` 映射为 `ApiError`
- 命令层统一 `blocking(spawn_blocking)` 跑 SQLite

---

## 4. 当前 API 设计

- 返回：`ApiResult<T> = { ok: true, data } | { ok: false, error }`
- 错误码：`not_found / invalid_argument / unauthorized / rate_limited / timeout / internal / unavailable / database_too_new`
- 命令命名：`<domain>_<action>`，Rust `#[tauri::command]`，参数 snake_case ↔ 前端 camelCase
- 事件：目前**没有任何 Tauri event 使用**（无 `app.emit`），AI 流式事件将是第一处

---

## 5. SAGE Intelligence 实际能力（读源码，2026-11-22）

### 5.1 可锁定的版本

| 项 | 实际值 |
|---|---|
| 仓库 | `qiufengquq/sage-intelligence`（public） |
| 默认分支 | `main` |
| Release / Tag | **0 个 release、0 个 tag** |
| 提交数 | 2 |
| 最新 commit | `33be312c6949afa7f20051ec40fd520002ec5da6`（Add Apache License 2.0） |
| workspace 版本 | `0.1.0` |
| 要求 Rust | **1.85** |
| 许可证 | `MIT OR Apache-2.0`（仓库根另有 Apache-2.0 LICENSE） |

→ 只能锁 commit：`rev = "33be312c6949afa7f20051ec40fd520002ec5da6"`。

### 5.2 crate 结构

| crate | 作用 | 依赖 |
|---|---|---|
| `ai-core` | AI 基础设施内核 | thiserror / serde / tokio / tracing / reqwest(rustls) / futures-core / rusqlite(bundled) |
| `ai-core-tauri` | 事件信封桥接 | **只依赖 ai-core + serde，不依赖 tauri** |

`ai-core` 模块（源码 `lib.rs` 实测）：`error`、`ids`、`secret`、`storage`、`config`、
`observability`、`async_utils`、`provider`、`usage`、`output`、`tool`、`memory`、
`embedding`、`context`、`cache`。

### 5.3 宿主可用的关键 API

```rust
// 配置（不可变快照 + 热更新）
ConfigStore::new(AiCoreConfig) -> CoreResult<Self>
ConfigStore::snapshot() -> Arc<ConfigSnapshot>   // .config() / .version()
ConfigStore::apply(AiCoreConfig) -> CoreResult<Arc<ConfigSnapshot>>

// Provider 运行时
ProviderRuntime::new(Arc<ConfigStore>) -> CoreResult<Self>
ProviderRuntime::complete(AiRequest, &CallContext) -> CoreResult<AiResponse>
ProviderRuntime::stream(AiRequest, &CallContext) -> CoreResult<AiStream>
ProviderRuntime::embed(EmbeddingRequest, &CallContext) -> CoreResult<EmbeddingResponse>
ProviderRuntime::with_observer(Arc<dyn AttemptObserver>) -> Self

// 输出引擎（统一事件 + 结构化输出 + 工具轮次）
OutputEngine::new(Arc<ProviderRuntime>) -> Self
OutputEngine::complete(AiRequest, &CallContext) -> CoreResult<OutputOutcome>
OutputEngine::stream(AiRequest, &CallContext) -> CoreResult<OutputStream>

// 请求
AiRequest::new(messages) / AiRequest::user_prompt(text)
  .with_model(ModelSpec) / .with_timeout(Duration) / .with_temperature(f32)

// Provider 配置
ProviderId::new("deepseek") -> CoreResult<ProviderId>
ProviderConfig::new(ProviderId, ProviderProtocol::OpenAiCompatible)
  .with_base_url(url)
  .with_api_key(key)                       // 内部是 SecretString
  .with_model(ModelConfig::new("deepseek-chat"))
  .with_default_model("deepseek-chat")
```

### 5.4 事件协议（`ai-core-tauri`）

```rust
pub trait EventEmitter: Send + Sync { fn emit(&self, envelope: EventEnvelope); }
pub struct EventEnvelope { pub event: String, pub payload: serde_json::Value }
pub const EVENT_PREFIX: &str = "ai-core://";

pub async fn start_stream(&OutputEngine, AiRequest, &StreamCommand, &dyn EventEmitter)
    -> Result<OutputOutcome, CommandError>
pub async fn complete(&OutputEngine, AiRequest, &StreamCommand)
    -> Result<OutputOutcome, CommandError>
pub fn outcome_error(&OutputOutcome) -> Option<CommandError>
```

事件名（11 个，稳定）：
`started`、`text-delta`、`reasoning-delta`、`tool-call-started`、`tool-call-delta`、
`tool-call-completed`、`tool-call-executed`、`structured-output`、`usage-updated`、
`error`、`completed`

取消语义：`StreamCommand::cancel()`（持 `CallContext` 的 `CancelToken`），
取消后流在下一个 poll 停止，`completed.interrupted = true` 且带部分文本。

### 5.5 能力映射（诚实标注）

| 契约项 | Intelligence 现状 | 客户端计划 |
|---|---|---|
| Runtime initialize / status / shutdown | `ConfigStore` + `ProviderRuntime` 可覆盖 | 实现 |
| Runtime version | 无专门 API | 编译期注入（build.rs 读 Cargo.lock） |
| Provider 配置 / 默认模型 | `ProviderConfig` 完整支持 | 实现 |
| Provider 连接检测 | 无专门 API | 用一次极短 `complete` 探测（标注为探测而非官方 API） |
| Agent 执行 / 状态 / 取消 | **无 Agent API**（只有 output 引擎） | 讲成"AI 请求 + 取消"，不叫 Agent；契约里 Agent 标 planned |
| Workflow | 只有 `RoundLoop`（工具轮次循环） | 标 planned |
| Conversation 创建/读取 | `memory::MemoryStore`（FTS5 + 向量） | 标 planned（客户端暂不落会话） |
| Memory 查询/更新/清理 | `MemoryStore` 有 | 标 planned |
| 流式事件 | `start_stream` + 11 事件 | 实现 |
| 用量统计 | `usage` 模块（SQLite 独立库） | 可选接入（标 planned 更稳） |

---

## 6. 集成位置（最小侵入）

### 6.1 Rust

```
src-tauri/src/
  ai/                        ← 新增（不叫 services，避免与现有事务编排混淆）
    mod.rs                   模块出口
    state.rs                 AiState { status, store, runtime, engine, streams }
    config.rs                Provider 配置的装配与脱敏视图
    commands.rs              ai_status / ai_provider_* / ai_prompt / ai_cancel
    emitter.rs              EventEmitter 的 Tauri 实现（app.emit）
    version.rs              编译期版本信息（build.rs 注入）
  state.rs                   AppState 增加 `ai: Arc<AiState>`（同一 AppState，不新建）
  commands/mod.rs            导出 ai
  lib.rs                     注册新命令
  build.rs                   读 Cargo.lock 注入 ai-core 版本/commit
```

### 6.2 前端

```
src/services/api/ai.service.ts     ← 新增（与 user/training/review 同级）
src/types/ai.ts                    ← 新增 DTO
src/services/api/contracts.ts       ← 增加命令名与 ready 状态
src/features/settings/ai-section.tsx ← 新增分区组件，插进现有 SettingsPage
src/features/settings/settings-page.tsx ← 只加一个 section + 版本号改为动态
```

不改 `transport.ts`、`serve`、`ApiResult`、`ErrorState` 等既有契约。

---

## 7. 需要新增 / 修改的代码

### 7.1 新增

| 文件 | 内容 |
|---|---|
| `src-tauri/src/ai/mod.rs` 等 6 个 | AI 状态、命令、事件、版本 |
| `src-tauri/src/database/migrations/0003_ai.sql` | `ai_providers` 表（不含明文密钥） |
| `src/services/api/ai.service.ts` | 统一 AI 入口（含事件订阅与取消） |
| `src/types/ai.ts` | `AiStatus` / `AiProviderView` / `AiEvent` 等 |
| `src/features/settings/ai-section.tsx` | 设置页 AI 管理区 |
| `docs/AI_INTEGRATION.md` | 集成契约（含 planned 标注） |
| `docs/RELEASE_NOTES_1.0.1.md` | 发布说明 |

### 7.2 修改

| 文件 | 改动 |
|---|---|
| `package.json` | `version: 1.0.1` |
| `src-tauri/Cargo.toml` | `version = "1.0.1"`、`rust-version = "1.85"`、新增 git 依赖 |
| `src-tauri/tauri.conf.json` | `version: 1.0.1` |
| `src-tauri/build.rs` | 注入 `SAGE_AI_*` 环境变量 |
| `src-tauri/src/state.rs` | `AppState { db, ai }` |
| `src-tauri/src/lib.rs` | 注册 8 个 ai 命令 |
| `src-tauri/src/database/mod.rs` | `SCHEMA_VERSION = 3` + 测试表数断言 36 → 37 |
| `src-tauri/src/commands/app.rs` | `version: env!("CARGO_PKG_VERSION")`，stage 更新 |
| `src/features/settings/settings-page.tsx` | 版本号动态、新增 `ai` 分区 |
| `docs/API_CONTRACTS.md` / `README.md` / `FRONTEND_ARCHITECTURE.md` | 同步 |

---

## 8. 潜在冲突与阻塞

### 8.1 阻塞项（必须先解决）

**B1. 本会话没有命令执行能力。**
当前环境无终端工具，无法运行 `npm run typecheck / lint / test / build`、`cargo check`、
`cargo test`、`tauri build`。而本任务要求以真实测试结果交付。
→ 需要恢复命令执行能力，或由使用者本地执行并回传输出。

**B2. Rust 版本门槛。**
`ai-core` 要求 `rust-version = 1.85`；客户端当前声明 1.77。
若本机工具链 < 1.85，Git 依赖直接编译失败。
→ 需要 `rustc ≥ 1.85`（`rustup update stable`），并把客户端 `rust-version` 提到 1.85。

**B3. 无 tag 可锁。**
上游 0 release / 0 tag，只能锁 commit
`33be312c6949afa7f20051ec40fd520002ec5da6`；后续每次跟进都要手改 rev。

### 8.2 需要确认的架构决策

**D1. AI 配置（非密钥）落库方式**
- 方案 A（推荐）：新增迁移 `0003_ai.sql` 建 `ai_providers` 表；`SCHEMA_VERSION` 2→3；
  同步修改表数断言。
- 方案 B：塞进 `app_settings` 的 `ai.*` 键。零迁移，但 Provider 是多行结构，
  会被压成 JSON 字符串，且 `settings_update` 的合并逻辑与服务层语义不匹配。
→ 默认按 A 实施；若不允许新增表，则退回 B。

**D2. API Key 存放**
- `ProviderConfig.api_key` 是 `SecretString` 且 `#[serde(skip_serializing)]`，
  但客户端仍需持久化，否则每次启动都要重输。
- 方案（推荐）：存独立文件 `app_data_dir/ai/provider-keys.json`，权限 0600，
  **只由 Rust 读写，永不回传前端**；前端只拿到 `hasApiKey: boolean`。
- 明确不做：明文写业务 SQLite、写日志、写进 `tauri.conf` 或前端 bundle。
- 已知限制：未使用 OS 凭据库（Windows Credential Manager），标注在 RELEASE_NOTES。

**D3. AI 存储目录**
`ai-core` 强制独立存储布局（`StorageConfig.root`，默认 `ai-core-data`）。
计划：`app_data_dir()/ai-core-data/`，与 `sage.db` 物理分离，满足"不共享业务库"。
不迁移、不复用业务库连接。

**D4. 命令粒度**
建议 4 组 8 个命令（全部走 `COMMANDS` + `COMMAND_STATUS`）：
`ai_status`、`ai_provider_list`、`ai_provider_upsert`、`ai_provider_test`、
`ai_prompt`（非流式）、`ai_stream_start`、`ai_stream_cancel`、`ai_version`

**D5. 事件命名**
直接转发 `ai-core://*` 原生事件名，另加一个客户端自定义
`sage-ai://stream-finished` 用于把 `OutputOutcome`（含 XP 无关的 usage/finishReason）
交回前端。不重命名框架事件，避免两套协议。

---

## 9. 集成实施计划

| 阶段 | 内容 | 验证 |
|---|---|---|
| P0 | 恢复命令执行；确认 `rustc ≥ 1.85` | `rustc -V` / `cargo -V` |
| P1 | 版本号统一 1.0.1 + build.rs 版本注入 + 应用内版本动态 | `npm run typecheck`、`cargo check` |
| P2 | 迁移 0003 + `ai_providers` 仓储 + `AiState` 初始化（不联网） | `cargo test --lib`（表数断言已更新） |
| P3 | AI 命令组 + 事件转发 + 取消 + 密钥文件 | `cargo check` + 手测 `ai_status` |
| P4 | 前端 `ai.service.ts` + 设置页 AI 区 | `npm run typecheck` / `lint` / `test` |
| P5 | 文档 + 构建 | `npm run tauri:build`，检查 MSI/NSIS 产物 |

每阶段结束必须编译；不通过不进入下一阶段。

---

## 10. 明确不做

- 不新建 Dashboard / Analytics 的 Rust 实现
- 不实现独立 AI 进程 / Sidecar / 下载更新（架构已正式排除，不是"预留"）
- 不接 Agent / Workflow / Conversation 的 UI（框架侧也没有对应 API）
- 不改现有 UI 设计语言与目录结构
- 不替换 `transport.ts` / `ApiResult` / 现有错误码体系
- 不把 `ai-core` 源码复制进本仓库

---

## 11. 审查后进展（1.0.1 实施记录）

| 编号 | 审查时状态 | 实际状态 |
|---|---|---|
| B1 无命令执行能力 | 阻塞 | 已恢复；`typecheck / lint / test / build / cargo test / tauri:build` 均已实际执行 |
| B2 Rust 1.85 | 阻塞 | 本机 `rustc 1.99.1`；`rust-version` 已提升到 1.85 |
| B3 无 tag 可锁 | 阻塞 | 按建议锁 commit `33be312c6949afa7f20051ec40fd520002ec5da6` |
| D1 AI 配置落库 | 待确认 | 方案 A：`0003_ai.sql` 新增 `ai_providers`，`SCHEMA_VERSION` 3，表数 37 |
| D2 API Key 存放 | 待确认 | 独立文件 `app_data_dir/ai/provider-keys.json`，前端只见 `hasApiKey` |
| D3 AI 存储目录 | 待确认 | `app_data_dir/ai-core-data/`，与 `sage.db` 物理隔离，懒创建 |
| D4 命令粒度 | 8 条 | 已实现 8 条（`ai_status / ai_version / ai_provider_upsert / ai_provider_delete / ai_provider_test / ai_complete / ai_stream_start / ai_stream_cancel`） |
| D5 事件命名 | 保留 `ai-core://*` | 已实现；另加客户端终态事件 `sage-ai://stream-finished` |
| Agent / Workflow / Conversation / Memory | 上游无 API | 保持 `planned`，未提供假接口 |

验证数据见 `docs/RELEASE_NOTES_1.0.1.md` §7。
