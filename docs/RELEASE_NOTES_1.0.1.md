# SAGE Client 1.0.1 — Desktop 发布说明

发布日期：2026-11-22
发布渠道：`stable`
构建平台：`x86_64-pc-windows-msvc`

---

## 1. 版本

| 项 | 值 |
|---|---|
| SAGE Client | `1.0.1` |
| SAGE Intelligence | `0.1.0`（源码已合并为 workspace 成员，基线 commit `b9990e9`） |
| AI Integration Protocol | `1` |
| 集成方式 | `library`（随客户端编译，无独立进程） |

版本号唯一来源：`package.json`（前端）、`src-tauri/Cargo.toml`（Rust 与安装包）、
`src-tauri/tauri.conf.json`（Tauri bundle）。应用内展示通过 `__APP_VERSION__`（Vite 注入）
与 `CARGO_PKG_VERSION` 读取，**代码中不再有硬编码版本字符串**。

AI 框架版本与 commit 由 `src-tauri/build.rs` 在构建时从 `Cargo.lock` 解析，
通过 `cargo:rustc-env` 注入；单元测试 `ai::version::tests::injected_versions_are_real`
会校验注入成功（防止静默降级为 `unknown`）。

---

## 2. 新增

### 2.1 SAGE Intelligence 集成（Library 模式）

- 依赖 `ai-core` / `ai-core-tauri`，锁定到具体 commit（上游 0 tag / 0 release）。
- 新增 `src-tauri/src/ai/`：`state`（配置快照 / 输出引擎 / 流句柄）、`config`（记录 → 框架配置）、
  `commands`（8 个 Tauri 命令）、`emitter`（事件转发）、`secrets`（密钥文件）、`version`（版本信息）。
- `AppState` 扩展为 `{ db, ai }`，**没有**第二套数据库连接管理器：AI 需要读配置时借用业务库连接。
- AI 初始化失败不阻塞启动，状态与原因写入 `AiStatus.lastError`。

### 2.2 命令

`ai_status`、`ai_version`、`ai_provider_upsert`、`ai_provider_delete`、`ai_provider_test`、
`ai_complete`、`ai_stream_start`、`ai_stream_cancel`。契约见 `docs/AI_INTEGRATION.md`。

### 2.3 事件

转发框架事件 `ai-core://started｜text-delta｜reasoning-delta｜tool-call-*｜structured-output｜usage-updated｜error｜completed`，
另加客户端终态事件 `sage-ai://stream-finished`。

### 2.4 前端

- `src/services/api/ai.service.ts`：统一入口（状态、Provider、请求、流式、取消、事件订阅）。
- `src/types/ai.ts`：契约类型 + 事件名常量。
- `src/features/settings/ai-section.tsx`：设置页「AI 集成」分区（框架信息、运行状态、
  Provider 管理、诊断信息）。沿用既有设计系统，未新增 UI 语言。

---

## 3. 修改

| 文件 | 改动 |
|---|---|
| `package.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json` | 版本 → `1.0.1` |
| `src-tauri/Cargo.toml` | `rust-version` → `1.85`；AI 依赖改为 `path` 指向 `crates/*` |
| 根 `Cargo.toml` | 新增 workspace（`src-tauri` + `crates/ai-core` + `crates/ai-core-tauri`） |
| `src-tauri/build.rs` | 注入 AI 版本 / commit / 构建时间 / 构建目标 |
| `vite.config.ts` + `src/vite-env.d.ts` | 注入并声明 `__APP_VERSION__` |
| `src-tauri/src/state.rs` | `AppState` 增加 `ai`，初始化 AI 状态 |
| `src-tauri/src/lib.rs` | 注册 8 个 AI 命令；退出时置 AI 状态为 `shutdown` |
| `src-tauri/src/commands/app.rs` | `version` 改为 `env!("CARGO_PKG_VERSION")` |
| `src/services/api/contracts.ts` | 新增 8 条命令并标记 `ready` |
| `src/features/settings/settings-page.tsx` | 新增「AI 集成」分区入口；版本号改为动态 |
| `tests/lib/contracts.test.ts` | 修正 `review_queue` 状态（2.3 遗留）；新增 AI 命令断言 |
| `src-tauri/src/database/mod.rs` / `backup.rs` | 表数断言 36 → 37 |

---

## 4. 数据库迁移

| 版本 | 脚本 | 内容 |
|---|---|---|
| 3 | `0003_ai.sql` | 新增 `ai_providers`（仅非密钥字段）与索引 |

- `SCHEMA_VERSION` 2 → 3，只增表不改表，旧库自动前进，不丢学习数据。
- API Key **不写入**业务库：存放于 `app_data_dir/ai/provider-keys.json`，只由 Rust 读写。
- AI 框架自身存储（memory / usage / cache）在 `app_data_dir/ai-core-data/`，与 `sage.db` 物理隔离；
  该目录为懒创建（首次真正使用 AI 时才出现）。

---

## 5. 依赖与体积

新增依赖（间接）：`tokio`、`reqwest`（rustls-tls）、`futures-core`、`tracing` 等。

| 产物 | 1.0.0 前 | 1.0.1 |
|---|---|---|
| `sage.exe` | 约 5.1 MiB | 见下方构建结果 |
| MSI | 2.93 MiB | **3.73 MiB** |
| NSIS `-setup.exe` | — | **2.97 MiB** |

体积增长来自 TLS 栈（rustls/ring）与 HTTP 客户端，属于集成 AI 的必然成本。

---

## 6. 构建要求

| 要求 | 说明 |
|---|---|
| Rust | **≥ 1.85**（`ai-core` 的 MSRV；本机 1.99.1 通过） |
| Node.js | ≥ 20 |
| 网络 | 首次构建需拉取 crates.io 依赖（框架源码已随仓库，无需 git 网络） |
| 平台 | Windows x64（WebView2 系统自带） |

---

## 7. 验证结果（实际执行）

| 检查 | 命令 | 结果 |
|---|---|---|
| 类型检查 | `npm run typecheck` | 通过（exit 0） |
| Lint | `npm run lint` | 0 warning / 0 error（156 文件） |
| 前端测试 | `npm run test` | 15 文件 / **87 passed** |
| Rust 测试 | `cargo test --lib` | **107 passed** |
| 前端构建 | `npm run build` | 成功 |
| 桌面构建 | `npm run tauri:build` | 成功，release 3m01s，无警告 |

启动验收（隔离数据库 `SAGE_DB_PATH=.acceptance-101/sage.db`）：

- 应用启动，窗口 `Sage`，进程 Responding。
- 迁移到 `user_version=3`，**37 张表**，3 条迁移记录。
- 学习数据完好：36 实体 / 36 状态行 / 1 用户 / 8 项设置。
- 写入一条「已配置但无密钥」的 Provider 后重启：应用正常启动（AI 初始化不因缺密钥而失败）。
- `ai-core-data/` 与 `ai/` 目录在未使用 AI 前不存在（无隐式写盘）。

---

## 8. 已知限制

1. **真实 AI 请求未端到端验证**：本机没有可用的 Provider API Key，`ai_complete` /
   `ai_stream_start` / `ai_stream_cancel` 的网络往返与流式事件需要填入真实密钥后手动验证。
2. **API Key 未接入 OS 凭据库**：以 `0600` 语义的独立文件存放（Windows 下依赖目录 ACL），
   后续可替换为 Windows Credential Manager 而不改数据模型。
3. **Agent / Workflow / Conversation / Memory 标记为 planned**：上游只有 Provider 路由、
   输出引擎、工具轮次与记忆原语，没有高层 Agent 抽象；客户端不提供假接口。
4. **工具调用未启用**：客户端不注册 `ToolRegistry`，模型无法触发工具（安全边界）。
5. **AI 用量看板未接入**：框架已写 `usage.sqlite3`，UI 未读取。
6. **架构决策：不采用 Sidecar / 独立 Runtime / 本地 HTTP 服务**。AI 与客户端同进程、
   同一个可执行文件；升级通过更新 Cargo 依赖并重新构建实现，没有独立下载/更新/回滚。
   `integration_mode` 恒为 `library`。
7. 首次构建需拉取 crates.io 依赖；框架源码已随仓库，不再依赖 git 网络。

---

## 9. 升级提示

- 已有 1.0.0 数据可直接升级：启动时自动执行 `0003_ai` 迁移。
- 未配置 Provider 时，所有 AI 入口显示"未配置"，学习功能不受影响。
- 回滚到 1.0.0 前请先备份（1.0.0 无法识别 `user_version=3`，会报 `database_too_new`）。
