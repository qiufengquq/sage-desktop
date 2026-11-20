# 第二阶段实施计划（阶段 A 交付）

> 前置文档：`DATA_MODEL_REVIEW.md`（现状与缺陷）、`DATABASE_DESIGN.md`（表结构）、`RUST_DATABASE_ARCHITECTURE.md`（驱动/连接/迁移/事务）。
> 本文只做任务拆分、文件清单、验收标准、风险与待确认问题。**阶段 A 结束时不动业务代码。**

---

## 0. 一句话方案

把"前端整份 `LearningDb` 快照 + localStorage" 换成 "**Rust 侧 SQLite 权威存储 + 事务化业务命令**"，
前端保持 `React → services/api → invoke` 的分层与全部 DTO 形状不变，Mock 适配器继续用于开发与测试。

```
现状                                 目标
页面                                 页面（不变）
  ↓ useQuery                           ↓ useQuery（不变）
services/api/*.service.ts            services/api/*.service.ts（入参对齐，DTO 不变）
  ↓ serve()                            ↓ serve()
  ├─ mock: localStorage 整份 JSON  →   ├─ mock: 内存仓储（仅开发/测试，不落地）
  └─ backend: unavailable 占位     →   └─ backend: Tauri Command → SQLite（事务）
```

---

## 1. 阶段划分与里程碑

| 阶段 | 内容 | 出口标准 | 预估 |
|---|---|---|---|
| **A** | 审查 + 设计（本次） | 4 份文档 + 你确认 | 已完成 |
| **B** | 数据库基础设施 | 库能建、能迁移、能备份；`state_repository` 打通一条端到端链路 | 3–4 天 |
| **C** | 业务持久化（7 个模块，逐模块迁移） | 每个模块的仓储 + service + 算法有测试 | 8–12 天 |
| **D** | 命令替换（逐条去掉 unavailable） | `backend` 模式下 7 个页面全部真实可用 | 3–4 天 |
| **E** | 验收 | 五道关卡全绿 + 迁移/集成测试通过 | 1–2 天 |

顺序不可颠倒：B 建立地基，C 逐个模块落地，D 才切前端数据源。**不要一次性全替换**。

---

## 2. 阶段 B：数据库基础设施

### B1 依赖与错误

| 文件 | 变更 |
|---|---|
| `src-tauri/Cargo.toml` | 加 `rusqlite(bundled,backup)`、`chrono`、`sha2` |
| `src-tauri/src/error.rs` | 加 `impl From<rusqlite::Error> for AppError`（映射见架构文档 §7） |

### B2 数据库核心

| 文件 | 内容 |
|---|---|
| `src-tauri/src/database/mod.rs` | `Database::{open, path, with_conn, with_tx}`；`with_tx` 用 `TransactionBehavior::Immediate` |
| `src-tauri/src/database/pragmas.rs` | WAL / synchronous / foreign_keys / busy_timeout / cache_size |
| `src-tauri/src/database/migrate.rs` | runner：备份 → 逐条执行 → 事务内写 `schema_migrations` + `user_version` |
| `src-tauri/src/database/migrations/0001_init.sql` | `DATABASE_DESIGN.md` §3 的全部 DDL |
| `src-tauri/src/database/backup.rs` | `backup()` / `restore()` / `integrity_check()` / `vacuum()` |
| `src-tauri/src/database/content.rs` | 内容包同步（UPSERT + `content_pack_version`） |
| `src-tauri/src/state.rs` | `AppState { db: Arc<Database> }`，删除内存 KV |
| `src-tauri/src/lib.rs` | `setup` 里初始化数据库：取 `app_data_dir` → open → migrate → content sync |
| `src-tauri/src/content/content_pack.json` | 由前端导出（见 B4） |

### B3 模型与首个仓储（试点）

| 文件 | 内容 |
|---|---|
| `src-tauri/src/models/mod.rs` + `entity.rs`/`user.rs` | 第一批 serde 结构，`#[serde(rename_all = "camelCase")]` |
| `src-tauri/src/repositories/state_repository.rs` | `load` / `upsert` / `due_before` / `reset_all` |
| `src-tauri/src/repositories/user_repository.rs` | `profile_get/update`、`settings_get/update`（点分键 UPSERT） |
| `src-tauri/src/commands/user.rs` | `settings_get/update` 从内存 KV 改为读库；`user_profile_get/update` 去掉 unavailable |
| `src-tauri/src/commands/app.rs` | `app_info.stage` 改为 `phase-2-sqlite` |

**试点的作用**：用"设置读写"这一条最薄的链路验证 连接、迁移、事务、错误映射、serde 命名、线程模型 六件事。这一步通了，后面的模块都是重复劳动。

### B4 内容导出脚本

| 文件 | 内容 |
|---|---|
| `scripts/export-content.mjs`（新增） | 读 `src/services/mock/*` 与 `src/types/*`，输出 `src-tauri/src/content/content_pack.json`；`--check` 模式只校验不写 |
| `package.json` | `"export:content": "node scripts/export-content.mjs"`、`"check:content": "node scripts/export-content.mjs --check"` |

导出内容：36 个实体 + 释义/例句/错误 + 关系边 + 43 道题 + 12 个训练入口 + 3 个测评模块/15 个分项。**不含学习态**（掌握度只在种子首次写入时按 `seed.ts` 的规则生成一次，之后由业务写入）。

### B5 契约修补（前端）

| 文件 | 变更 |
|---|---|
| `src/services/api/contracts.ts` | 补 `reviewQueue/reviewSubmit`（Rust 侧同步注册）；新增 `dbMaintenance`、`learningRecordsPage`（若 Q6 通过） |
| `src/services/api/training.service.ts` | `submitSession` 的 `args` 改为后端可直接判分的形式（见 Q4） |
| `src/services/api/assessment.service.ts` | `args` 补 `submissionId` |
| `src/services/api/review.service.ts` | 入参补可选 `submissionId`（D6） |

### B 出口验收

- [ ] 首次启动自动建库、跑完 `0001`，`user_version = 1`
- [ ] 第二次启动不重复迁移，`content_pack` 不重复插入
- [ ] 迁移脚本被改动后启动报 checksum 错误（不静默通过）
- [ ] `settings_update` 在 backend 模式下生效，重启后保留
- [ ] `cargo test` 覆盖：迁移幂等、事务回滚、错误映射
- [ ] 数据库文件出现在 `app_data_dir`，含 `-wal`/`-shm`
- [ ] 备份文件可生成、可恢复、恢复后可读

---

## 3. 阶段 C：业务持久化（逐模块）

顺序按"依赖少 → 依赖多"排列，每个模块独立可验收。

### C1 知识库（内容 + 学习态读取）

| 层 | 文件 |
|---|---|
| repo | `repositories/entity_repository.rs`（`knowledge_index` 查询、详情、邻居、图谱） |
| service | `services/knowledge_service.rs`（内容 + 状态合成，对应 `knowledge.service.ts:31-69` 的 `merge*`） |
| cmd | `commands/knowledge.rs` 四条命令去 `unavailable` |
| 算法 | 图谱坐标 `seedPosition()` 移植到 `algorithms/layout.rs` |
| 验收 | 36 个节点、边数与前端一致；`?id=` 深链打开详情正确；筛选（kind/difficulty/mastery/keyword）结果与 mock 模式一致 |

### C2 训练（写路径，第一个事务）

| 层 | 文件 |
|---|---|
| 算法 | `algorithms/grading.rs`（`lib/grading.ts`）、`xp.rs`、`mastery.rs`、`srs.rs`、`selection.rs` |
| repo | `repositories/training_repository.rs`、`mastery_repository.rs` |
| service | `services/training_service.rs`（`start` 选题 + `submit` 事务） |
| cmd | `commands/learning.rs` 的 `training_options/start/submit` |
| 事务 | `BEGIN IMMEDIATE` → 幂等查 → 写 session/answers/kind_scores/entities → UPSERT states → 写 mastery_events → 标记 plan_item → `db_revision+1` → COMMIT |
| 验收 | 提交后：记录入库、掌握度上升、`due_at` 有值、XP 增加、首页数字变化；重复提交不重复计分；异常时整体回滚 |

### C3 测评

| 层 | 文件 |
|---|---|
| 算法 | `algorithms/ability.rs`（`lib/algorithms/ability.ts`）、`levelOfScore` |
| repo | `repositories/assessment_repository.rs` |
| service | `services/assessment_service.rs`（判分 → 写 attempt/sections/answers → 写 states → 触发计划重算） |
| cmd | `assessment_modules` / `assessment_submit` |
| 验收 | 3 个模块可提交；`lastAttempt` 来自真实记录；能力画像随测评变化 |

### C4 复习

| 层 | 文件 |
|---|---|
| repo | `repositories/review_repository.rs` |
| service | `services/review_service.rs`（队列 = `due_at <= now`；提交 = 调度 + 日志 + 事件 + 计划项） |
| cmd | `commands/review.rs`（**新增文件**，注册两条命令） |
| 验收 | 队列排序与 mock 一致；`preview` 四档间隔与 TS 一致（parity test）；同一 `submissionId` 重复提交只记一条；"再来一轮"取到的是新队列 |

### C5 学习计划

| 层 | 文件 |
|---|---|
| 算法 | `algorithms/planner.rs`（`lib/algorithms/planner.ts`） |
| repo | `repositories/plan_repository.rs`（版本切换：旧版 `superseded`，新版 `active`） |
| service | `services/plan_service.rs`（读画像/到期/正确率 → 生成 → 落库） |
| cmd | `plan_get/regenerate/apply_difficulty/toggle_item` |
| 验收 | 生成结果可解释（每条 advice 带 evidence）；`today` 与 `week[0]` 是同一行数据；重新生成保留今天已完成项；只有一份 active 计划 |

### C6 学习总览与分析

| 层 | 文件 |
|---|---|
| 算法 | `algorithms/stats.rs`（`buildDailyStats/weeklyFrom/computeStreak/masteryBreakdown/forgettingCurveOf/timeBuckets/masteryTrendOf`） |
| repo | `repositories/analytics_repository.rs`（只取原始行，聚合在算法层） |
| service | `services/dashboard_service.rs` |
| cmd | `dashboard_snapshot` / `analytics_query` |
| 验收 | 首页 8 个数字与 mock 模式一致；分析页 7/14/30 天区间正确；`storageUsedKb` 改为真实文件大小 |

### C7 设置与资料（收尾）

- `local_data_summary`（真实计数 + 文件大小）
- `local_data_clear`（`DATABASE_DESIGN.md` §5.2 的删除顺序 + 先备份）
- `sync_status_get`（`state: "idle"`、`pendingChanges: 0`、`storageUsedKb` 真实值）
- `sync_trigger` 继续明确返回 `not_implemented`（云同步不在本阶段）

---

## 4. 阶段 D：命令替换与前端切源

### D1 命令层收尾

- `learning.rs` 按域拆细（`plan.rs`、`assessment.rs`、`training.rs`），命令体保持 5–10 行的薄封装。
- 移除所有 `AppError::unavailable(...)` 占位（保留 `sync_trigger` 的 `not_implemented`）。
- `lib.rs` 的 `generate_handler!` 与 `contracts.ts` 做一次逐条对齐（当前缺 `review_*`，多 `app_*`）。

### D2 前端必要改动

| 文件 | 变更 | 原因 |
|---|---|---|
| `src/stores/learning-store.ts` | 新增 `mode: "mock" \| "backend"`；backend 模式下**不**从 localStorage 加载 `db`，只保留 `revision` | 避免"Mock 与真实库同时成为数据来源" |
| `src/stores/learning-store.ts` | 新增 `bumpRevision()`；写命令成功后调用 | backend 模式没有本地 db，`useQuery` 需要刷新信号 |
| `src/services/api/transport.ts` | `serve` 在 backend 模式成功后调用 `bumpRevision()`（或由各写命令显式调用） | 统一刷新，不散落在页面 |
| `src/services/api/*.service.ts` | 补齐后端所需 args（`sessionId/exercises/answers/submissionId`） | D13 |
| `.env.example` | 增加 `SAGE_DB_PATH`、`VITE_SAGE_DATA_SOURCE=backend` 的说明 | 联调 |

> `services/mock/*` 与 `services/data/*` **继续保留**：mock 模式仍是默认开发环境，也是 `npm test` 的数据源（`tests/integration/*` 依赖 `createMemoryRepository`）。

### D3 逐条切换验证

每条命令单独验证"mock 结果 = backend 结果"，不做批量切换：

```
设置 → 资料 → 知识库(4) → 训练(3) → 测评(2) → 复习(2) → 计划(4) → 总览/分析(2) → 本地数据(2)
```

---

## 5. 阶段 E：验收

### 5.1 必过关卡

```bash
npx oxlint src tests          # 0 warning / 0 error
npm run typecheck             # tsc -b 通过
npm run build                 # vite 构建通过
npm test                      # vitest（含新增 parity 测试）
npm run check:content         # 内容包与 TS 内容一致
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings   # 建议
```

### 5.2 集成测试（Rust）

| 用例 | 断言 |
|---|---|
| 训练闭环 | 会话/答案/状态/事件/XP 全部落库；首页数字变化 |
| 训练幂等 | 同 `submission_id` 两次提交 → 行数不变 |
| 训练回滚 | 注入第 3 条 INSERT 失败 → 库内无任何残留 |
| 复习闭环 | `due_at` 后移；`review_logs` + `mastery_events` 各 +1；计划项完成 |
| 复习幂等 | 同 `submission_id` 只记一条（新的 `ux_review_submission`） |
| 测评闭环 | 分项成绩与画像变化一致；旧计划 `superseded`、新计划 `active` |
| 数据清理 | 事件清空、状态重置、内容与实体保留 |
| 迁移 | 空库 → v1；带数据 → v2；版本过高 → 明确报错 |
| 备份/恢复 | 备份 → 改数据 → 恢复 → 数据回退到备份点 |

### 5.3 手工验收路径（backend 模式）

1. 首页 → 任一任务 → 进训练 → 完成 → 回首页数字变化
2. 复习 → 完成 5 张 → 总结页 → "再来一轮"取到新队列
3. 测评 → 提交 → 计划页建议与难度变化
4. 知识库 → 搜索/筛选 → 打开详情 → 关联跳转
5. 设置 → 改主题/提醒 → 重启应用后保留
6. 设置 → 清理数据 → 事件清空、知识库仍在
7. `db_maintenance` → 备份 → 恢复

---

## 6. 预计修改文件总览

### Rust（新增为主）

```
src-tauri/Cargo.toml                              改
src-tauri/src/lib.rs                              改（setup + 命令注册）
src-tauri/src/state.rs                            改（内存 KV → Arc<Database>）
src-tauri/src/error.rs                            改（From<rusqlite::Error>）
src-tauri/src/database/{mod,pragmas,migrate,backup,content}.rs    新
src-tauri/src/database/migrations/0001_init.sql   新
src-tauri/src/content/content_pack.json           新（生成物）
src-tauri/src/models/*.rs                         新（约 10 个）
src-tauri/src/repositories/*.rs                   新（约 9 个）
src-tauri/src/services/*.rs                       新（约 6 个）
src-tauri/src/algorithms/*.rs                     新（约 8 个）
src-tauri/src/commands/{user,learning,knowledge}.rs  改（去占位）
src-tauri/src/commands/{review,plan,assessment,training}.rs  新
```

### 前端（改动集中在服务层与 store）

```
src/services/api/contracts.ts              改（补 review_*、db_maintenance、learning_records_page）
src/services/api/transport.ts              改（backend 成功后 bumpRevision）
src/services/api/{training,assessment,review,user}.service.ts  改（args 对齐）
src/stores/learning-store.ts               改（mode + bumpRevision，backend 不加载本地库）
src/services/mock/*                        基本不动
src/services/data/*                        不动（mock 模式的领域层）
src/types/*                                不动（DTO 形状不变）
scripts/export-content.mjs                 新
tests/fixtures/parity/*.json               新（双端对照用例）
tests/parity/parity.test.ts                新
package.json                               改（export:content / check:content）
docs/API_CONTRACTS.md                      改（命令表状态、新命令）
docs/DATA_MODEL_REVIEW.md 等 4 份            本次新增
```

**不改**：`src/features/**`（页面）、`src/components/**`、`src/styles/**`、`src/app/**`、设计系统。

---

## 7. 风险与对策

| # | 风险 | 影响 | 对策 |
|---|---|---|---|
| R-1 | 算法双实现漂移 | 前端显示与后端裁定不一致，用户看到"分数对不上" | `tests/fixtures/parity/*.json` 双端跑同一组用例（架构文档 §8.3）；漂移即测试失败 |
| R-2 | 迁移失败导致用户数据不可用 | 最严重 | 迁移前强制备份；失败用备份回滚；`database_too_new` 拒绝启动而非改数据 |
| R-3 | 主线程被 SQL 阻塞 | 界面卡顿 | 全部命令 `async` + `spawn_blocking`；基准测试记录 p95 |
| R-4 | 契约不同步（args 名/结构） | invoke 静默丢参，功能"看起来不报错但没生效" | B5 先修契约；D3 逐条比对 mock/backend 返回；`API_CONTRACTS.md` 作单一清单 |
| R-5 | 计划项完成标记规则过宽（D10） | 一次复习打勾多条任务 | 明确改为"只标记 sort_order 最小的未完成项"，加测试 |
| R-6 | 时区与"今天" | 跨零点的用户看到错误日期的任务 | `plan_days.date` 存本地日历日；日统计在 Rust 用 `profile.timezone` 换算；UTC 存时间戳 |
| R-7 | 内容包与 TS 内容不一致 | 题库/词库两套 | `check:content` 进 CI；JSON 标注"生成物，勿手改" |
| R-8 | 数据库放在同步盘 | WAL 损坏 | 启动检测路径并警告；文档明确"不要放 OneDrive" |
| R-9 | 双开应用写同一库 | 计划/状态竞争 | 建议 `tauri-plugin-single-instance`（Q7）；不加则依赖 `busy_timeout` 串行化 |
| R-10 | 首次迁移后旧 localStorage 数据丢失 | 用户历史消失 | 见 Q5：要么做一次性导入（推荐），要么明确声明"第二阶段从种子开始" |

---

## 8. 待确认问题（需要你拍板）

### 已确认（2026-11-14，"全选建议"）

| # | 结论 | 落地状态 |
|---|---|---|
| Q1 | Rust 为权威实现，TS 降级为 mock + 对照基准 | 阶段 C 按模块移植，parity 测试随模块交付 |
| Q2 | 采用**方案 B**：提交只传 `{sessionId, answers}`，后端从落库快照取题判分 | ✅ 已建表 `training_session_snapshots`（迁移 0001） |
| Q3 | `review_submit` 增加可选 `submissionId` | ✅ 已建 `ux_review_submission` 部分唯一索引；命令层阶段 C4 实现 |
| Q4 | 新增 `learning_records_page`、`db_maintenance` | 契约与命令在阶段 C7/D 落地（**现在不提前声明**，避免出现"声明了但没实现"的缺口） |
| Q5 | 旧 localStorage 数据一次性导入 SQLite | 阶段 D 的 D2 任务 |
| Q6 | `users` / `user_profiles` 保留拆分 | ✅ 已按拆分建表 |

### 可默认项（同样按建议执行）

Q7 单实例插件（阶段 D 接入）、Q8 `synchronous=NORMAL`（✅ 已配置）、Q9 单连接不建池（✅ 已实现）、
Q10 内容包进 git（✅ `content_pack.json` 已生成）、Q11 计划项只标一条（阶段 C5）、Q12 保留 5 版计划 / 10 份备份（✅ 备份已实现）。

### 可默认（不反对就按建议执行）

| # | 建议默认值 |
|---|---|
| Q7 | 引入 `tauri-plugin-single-instance` 防双开 |
| Q8 | `synchronous=NORMAL`（崩溃最多丢最后一两个事务，不损坏库） |
| Q9 | 单连接 `Mutex`，不上连接池；实测 p95 > 16ms 再议 |
| Q10 | 内容包 JSON 进 git（生成物也提交，保证 CI 与离线构建可复现） |
| Q11 | 计划项标记规则改为"只标记一条"（修 D10） |
| Q12 | 保留最近 5 版计划、最近 10 份备份 |

---

## 9. 明确不做（本阶段边界）

1. 不实现任何 AI 调用、不装模型 SDK、不写 Prompt、不做模型路由/记忆/Agent。
2. 不引入 `zod` 以外的校验依赖；AI 接口只留 `src/types/ai-contracts.ts` 契约 + 手写运行期校验（阶段 D 之后单独排期，不影响 SQLite 主线）。
3. 不改设计系统、不改页面组件、不改路由。
4. 不实现云同步（`sync_trigger` 继续返回 `not_implemented`）。
5. 不推倒重写前端数据层：`services/mock` 与 `services/data` 原样保留为 mock 实现。

---

## 10. 阶段 B 实施进度（2026-11-14）

已完成并验证（`cargo test`：26 passed）：

| B 任务 | 交付物 | 状态 |
|---|---|---|
| B1 依赖与错误 | `Cargo.toml` 加 rusqlite(bundled,backup)/chrono/sha2；`error.rs` 加 `From<rusqlite::Error>`、`database_too_new`、`corrupt` | ✅ |
| B2 数据库核心 | `database/{mod,pragmas,migrate,backup,content}.rs`；`migrations/0001_init.sql`（35 表 + 1 视图） | ✅ |
| B2 状态与启动 | `state.rs` 换成 `Arc<Database>`；`lib.rs` 在 `setup` 中初始化并做首启种子 | ✅ |
| B3 试点链路 | `repositories/{user,state}_repository.rs`；`commands/user.rs` 全部改为真实读写（7 条命令） | ✅ |
| B4 内容导出 | `scripts/{alias-loader,export-content}.mjs`；`content/content_pack.json`（36 实体 / 59 关系 / 40 题 / 12 入口 / 3 模块 15 分项） | ✅ |
| B5 契约修补 | `package.json` 加 `export:content`/`check:content`；`.env.example` 加 `SAGE_DB_PATH` | 部分（新命令按 Q4 延后） |

尚未做（按计划推后）：`dashboard/analytics/plan/assessment/training/knowledge` 命令仍在 `unavailable`；
`local_data_clear` 延后到 C7（需与计划重建同事务）；`review_*` 命令在 C4 注册。

## 11. 下一步

阶段 C 建议从 **C1 知识库** 开始（纯读，风险最低），再进入 C2 训练（第一条写事务）。
每完成一个模块即执行一次 `cargo test` + `npx oxlint src tests` + `npm run typecheck`。

---

## 12. 阶段编号对照（2.1 起启用）

原 A–E 编号用于阶段 A 的粗排期；自 2.1 起改用"2.x"编号，二者对应关系如下（**下文起以 2.x 为准**）：

| 2.x | 原编号 | 内容 | 状态 |
|---|---|---|---|
| **2.1** | A | 业务模型审查 + 统一数据模型 + SQLite 设计 + 构建就绪 | ✅ 本期交付 |
| **2.2** | B（剩余）+ D 前置 | 模型对齐：schema v2、契约修补、判分权收归、幂等统一 | 待启动 |
| **2.3** | C1/C4 | 复习闭环 + 空分项语义落地（`review_*`） | 待启动 |
| **2.4** | C5/C7 | 计划持久化 + 设置收尾（`plan_*`、备份、单实例） | 待启动 |
| **2.5** | C6 | 首页与分析读模型（`dashboard_snapshot`、`analytics_query`） | 待启动 |
| **2.6** | D + E | 前端切源与全量验收（backend 模式七页可用 + 打包链接验证） | 待启动 |

### 12.1 进度修正（覆盖 §10 的"尚未做"描述）

§10 写于 B 阶段结束时，此后 C1/C2/C3 已落地，实际状态为：

| 模块 | Rust 实现 | 命令状态 | 前端 backend 可用 |
|---|---|---|---|
| 知识库 | `knowledge_service` + `entity_repository` | 4 条已注册 | ✔（受 N+1 影响） |
| 训练 | `training_service` + 快照 + 服务端判分 | 3 条已注册 | ✔ |
| 测评 | `assessment_service` + 分项判分 | 2 条已注册 | ✔ |
| 复习 | **未实现** | `review_*` 未注册 | ✘ |
| 计划 | **未实现**（表结构预留） | 4 条 `unavailable` | ✘ |
| 首页/分析 | **未实现** | 2 条 `unavailable` | ✘ |
| 用户/设置 | `user_repository` | 8 条中 6 条可用 | 部分 |

测试基线：`cargo test` 80 项通过；前端 `vitest` 73 项通过；`typecheck` / `oxlint` / `vite build` 全绿（详见 `TAURI_BUILD_READINESS.md`）。

---

## 13. 2.2 – 2.6 详细排期

> 依赖关系是硬约束：**2.2 未完成不要开 2.3**（否则 schema v2 一变，复习服务要重写两遍）。
> 每个阶段结束的固定动作：`cargo test` + `npm test` + `npm run typecheck` + `npm run lint` + `npm run check:content`。

### 2.2 模型对齐与契约收口（预计 3–4 天）

**任务**

| # | 任务 | 产出 | 依赖 |
|---|---|---|---|
| 2.2.1 | 迁移 `0002_model_alignment.sql` | 分项角色列、删除 `plan_items.done`、训练入口查询索引、4 处叶子表外键（先清洗孤儿）、`row_version` 决策 | 无 |
| 2.2.2 | 判分权收归后端 | 拆 `ExerciseForPlayer`（无 `correct/answer/expected`）与 `ExerciseWithKey`；`training_submit`/`assessment_submit` 只收 `{exerciseId, value, elapsedMs}` | 无 |
| 2.2.3 | 幂等键统一 | 三个提交接口 `submissionId` 必填；返回 `{duplicate:boolean}` | 2.2.1 |
| 2.2.4 | 学习态单一事实源 | 删除前端内容实体上的 `mastery/state/dueAt/strength/reviewCount/lastReviewedAt`；`KnowledgeDetail` 改为 `{content, state}` 组合 | 无 |
| 2.2.5 | 时间模型统一 ✅ | 时区感知原语 + `offsetMinutes` 贯穿统计；Rust 侧本已 UTC ISO（毫秒仅存在于内部比较） | 无 |
| 2.2.6 | `transport` 改为按命令能力 | `CAPABILITIES: Record<CommandName, "ready"|"unavailable">`，未实现命令走 mock 而非报错 | 无 |
| 2.2.7 | 错误码修正 | `database_too_new` 独立 `code`，前端给"请升级"提示 | 无 |
| 2.2.8 | `mastery_events.source` 增加 `assessment`（前端类型 + 写入） | 修 N1 | 无 |
| 2.2.9 | 一致性测试 | `tests/integration/model-invariants.test.ts`：INV-1/2/3/7 | 2.2.1 |

**验收标准**
- `0002` 迁移在已有数据库与空库两种情况下都成功，失败可回滚到快照。
- 客户端无法影响判分：把 `correct/partialScore` 改成任意值，提交结果不变（Rust 单测）。
- `grep` 前端代码不再出现从内容实体读 `mastery` 的地方。
- 未实现命令不再抛 `unavailable` 给页面（按能力回退）。
- 全部测试与四道前端关卡通过。

**风险**：2.2.4 与 2.2.2 是**破坏性契约变更**，必须前后端一次性合入；建议先写类型、再改服务、最后改页面（页面只改取值路径，不改 UI）。

### 2.3 复习闭环 + 空分项语义（预计 3 天）

**任务**

| # | 任务 | 产出 | 依赖 |
|---|---|---|---|
| 2.3.1 | `review_service::{queue, submit}` | 队列读 `entity_states.due_at`，提交写 `review_logs` + `entity_states` + `mastery_events(source=review)` | 2.2.1 |
| 2.3.2 | 注册 `review_queue` / `review_submit` | `lib.rs` + `contracts.ts` 对齐 | 2.3.1 |
| 2.3.3 | 复习幂等 | `submission_id` + `ux_review_submission`（索引已存在） | 2.3.1 |
| 2.3.4 | "新词入队"规则 | `due_at IS NULL` 且 `review_count=0` 的实体按创建顺序入队，数量受每日预算限制 | 2.3.1 |
| 2.3.5 | 分项角色落地 | 内容包输出 `section_role`；`submit` 对 `summary` 分项跳过判分；UI 显示"评估汇总" | 2.2.1 |
| 2.3.6 | 计划勾选收窄 | 复习只标记 `sort_order` 最小的未完成复习项（修 D10） | 2.3.1 |
| 2.3.7 | 账本连续性测试 | INV-6/INV-8 | 2.3.1 |

**验收标准**
- 复习 20 张卡后：`review_logs` 行数 20、`entity_states.due_at` 全部未来、`mastery_events` 20 条 `source=review`。
- 同一 `submissionId` 重复提交，行数不变。
- 口语模块提交不再出现 `score=0` 的"评估汇总"步；总分与旧版一致（回归测试）。
- 验收后 `review_logs` 从"零写入"变为"唯一事实源"。

**风险**：SRS 参数（`ease`/`interval`）一旦写入就是历史事实，**先在 parity 测试里锁死 TS/Rust 同参**，否则老用户的 `mastery` 会出现不可解释的跳变。

### 2.4 计划持久化 + 设置收尾（预计 3 天）

| # | 任务 | 产出 | 依赖 |
|---|---|---|---|
| 2.4.1 | 计划规则引擎移植（或包一层） | Rust `plan_service::generate(draft)`；输入全部来自 L3/L4 | 2.2.5 |
| 2.4.2 | 实现 4 条 `plan_*` 命令 | `plan_get` / `plan_regenerate` / `plan_apply_difficulty` / `plan_toggle_item` | 2.4.1 |
| 2.4.3 | 版本化 | 旧 `active` → `superseded`；保留最近 5 版 | 2.4.2 |
| 2.4.4 | 完成率单点 | 读模型返回 `completion`；删除前端 6 处重算 | 2.4.2 |
| 2.4.5 | 周内 6 天占位显式化 | `plan_days.status = not_generated` | 2.4.1 |
| 2.4.6 | 备份接线 | 启动后静默备份、设置页导出/恢复、写 `last_backup_at`（修 `sync_status` 恒 offline） | 无 |
| 2.4.7 | `local_data_clear` | 单事务清空 + 重新种子 + 重建计划 | 2.4.2 |
| 2.4.8 | `tauri-plugin-single-instance` | 防双开写库 | 无 |

**验收标准**：计划页在 backend 模式可读可改；重生成后历史版本仍在库中；完成率只有一个来源；`sync_status` 能显示真实备份时间。

**风险**：2.4.1 若照搬 TS 规则引擎，容易出现"计划内容变化"的观感差异。**先用同一份输入跑双端对比（golden test），确认 7 天计划逐项一致**，再切前端。

### 2.5 首页与分析读模型（预计 2–3 天）

| # | 任务 | 产出 | 依赖 |
|---|---|---|---|
| 2.5.1 | `analytics_service` | 日/周统计、总量、正确率、streak、掌握度分布、趋势、遗忘曲线 | 2.3（需要 `review_logs`） |
| 2.5.2 | `dashboard_snapshot` | 今日计划 + 复习队列数 + XP/等级 + 最近 5 条记录 | 2.4 |
| 2.5.3 | `analytics_query(range)` | 参数化区间，返回结构与前端 `AnalyticsData` 一致 | 2.5.1 |
| 2.5.4 | XP/等级接线 | `total_xp` + `level_from_xp` 接入首页 | 2.5.2 |
| 2.5.5 | 删除假数据 | 移除 `mock/{learning,analytics,dashboard}.ts` 中的硬编码指标，只保留种子输入 | 2.5.3 |
| 2.5.6 | 口径回归测试 | 6 个指标各 1 个断言，前后端数值一致（金标准 fixture） | 2.5.3 |

**验收标准**：断网、清空 localStorage 后首页仍有正确数字；`analytics_query` 与旧前端算法在 fixture 上差值为 0；`grep` 前端不再有 `avgMastery`/`accuracy` 的自算实现。

**风险**：时间归日依赖 `profile.timezone`，需要 2–3 个跨时区 fixture（UTC+8 / UTC-5 / 跨月）。

### 2.6 前端切源与全量验收（预计 2–3 天）

| # | 任务 | 产出 | 依赖 |
|---|---|---|---|
| 2.6.1 | localStorage → SQLite 一次性导入 | 导入脚本 + 幂等保护 + 导入结果日志 | 2.5 |
| 2.6.2 | 逐页切 backend 验证 | 7 个页面 × 关键操作 | 2.2–2.5 全部 |
| 2.6.3 | 双端一致性（parity）全量 | `tests/fixtures/parity/*` 覆盖 11 种题型 + SRS + 掌握度 | 2.3 |
| 2.6.4 | 构建链接验证 | `npx tauri build --no-bundle` | 无 |
| 2.6.5 | 手工验收脚本 | 见 §5.3（训练→复习→计划→首页 闭环） | 2.6.2 |
| 2.6.6 | 文档回写 | `API_CONTRACTS.md` 命令状态、`DATABASE_DESIGN.md` v2 变更 | 全部 |

**验收标准（五道关卡）**
1. `cargo test` 全绿，且新增服务各自有事务/幂等/空分项测试。
2. `npm test` 全绿，集成测试覆盖 4 条主链路。
3. `typecheck` / `oxlint` / `vite build` 全绿。
4. backend 模式下 7 个页面无 `unavailable` 错误横幅。
5. `tauri build --no-bundle` 通过（发布打包另议）。

**风险**：切源是唯一会碰到真实用户数据的动作。**导入必须先备份、必须可重入、失败要能回到 localStorage 模式。**

---

## 14. 全局风险与对策（2.2–2.6 共用）

| # | 风险 | 影响 | 对策 |
|---|---|---|---|
| P-1 | 破坏性契约变更（2.2.2/2.2.4/2.2.3） | 前后端不同步导致页面白屏 | 一次性合入；先加类型后改实现；保留 mock 分支作为回归基准 |
| P-2 | 算法双实现漂移 | 分数/掌握度两端不一致 | 每阶段跑 parity fixture；Rust 为唯一权威，TS 仅作对照 |
| P-3 | 迁移失败损坏用户库 | 最严重 | 迁移前快照（已实现）+ 失败整体回滚 + `database_too_new` 拒绝启动 |
| P-4 | 派生列口径变更导致历史不可比 | 趋势图断裂 | 保留算法版本号（`app_metadata.algorithm_version`），历史按版本解释 |
| P-5 | 判分权收归后离线不可用 | 断网时无法作答 | 判分是纯本地计算，无网络依赖；`mock` 分支仅用于开发 |
| P-6 | 工作量大导致中途停机 | 页面功能不可回退 | 每阶段结束时工程必须处于"可运行 + 测试全绿"状态，禁止跨阶段半成品 |

---

## 15. 明确不做（2.2–2.6 边界）

1. 不做云同步、不做账号体系、不做 AI 调用（AI 边界见 `LEARNING_DATA_MODEL.md` §9）。
2. 不改 UI 布局、设计系统、路由与组件库。
3. 不引入连接池、不引入 ORM、不引入迁移框架（自研 runner 已够用）。
4. 不做多用户、不做权限模型。
5. 不删除前置文档 `DATA_MODEL_REVIEW.md`（保留阶段 A 的原始判断作为对照）。

---

## 16. 2.2 实施进度（2026-11-14）

### 16.1 已完成

| 任务 | 交付物 | 证据 |
|---|---|---|
| 2.2.1 迁移 v2 | `database/migrations/0002_model_alignment.sql`；`SCHEMA_VERSION = 2`；`MIGRATIONS` 登记 | `cargo test database::migrate` 4 项通过，含新增的 **v1→v2 升级不丢数据** 测试 |
| 2.2.1a 分项角色 | `assessment_sections.section_role`（`graded`/`summary`）；内容包导出 `section_role`；老库按"无题即 summary"回填 | `upgrades_v1_database_and_keeps_data` 断言 `sec_speak_eval = summary` |
| 2.2.1b 计划单标志 | 删除 `plan_items.done`，`done=1` 折进 `status` | 同测试断言行数不变、`status='done'` |
| 2.2.1c 历史外键 | `training_answers.exercise_id`、`training_session_entities.entity_id`、`assessment_answers.exercise_id`、`assessment_section_results.section_id` 补齐（`ON DELETE RESTRICT`） | `pragma_foreign_key_list` 断言 |
| 2.2.1d 查询确定性 | `option_item_count(kind, track)` + `ORDER BY sort_order LIMIT 1`；新增 `idx_training_options_kind_track` | 修正原计划的"kind 加 UNIQUE"错误判断 |
| 2.2.3 幂等统一 | `assessment_submit` 的 `submissionId` 改为**必填**；`AssessmentOutcome` / `ExerciseResult` 增加 `duplicate`；空键报 `invalid_argument` | Rust 新增 `submit_requires_submission_id`；前端 `assessment-flow` 断言 `duplicate=false/true` |
| 2.2.6 命令能力声明 | `contracts.ts` 新增 `COMMAND_STATUS`（15 ready / 10 planned）；`transport.serve` 未实现命令回退 mock | `tests/lib/contracts.test.ts` 3 项 |
| 2.2.7 错误码 | `database_too_new` 独立错误码 + 前端映射与用户提示 | `error.rs`、`api.ts`、`transport.ts` |
| 2.2.8 事件来源 | 前端 `MasteryEvent.source` 增加 `assessment`，测评写入不再记成 `training` | `learning-db.ts`、`learning-state.ts` |
| 2.2.5 时间模型 | `lib/date.ts` 新增时区原语（`isoDateIn / hourIn / startOfDayIn / addDaysIn / daysBetweenIn / lastNDaysIn / offsetMinutesForTimeZone`）；`buildDailyStats / weeklyFrom / timeBuckets / masteryTrendOf` 接受 `offsetMinutes`；统计入口统一从 `profile.timezone` 取偏移 | `tests/lib/date-timezone.test.ts` 6 项（东八区/纽约冬季、窗口滚动） |
| 2.2.2 判分权收归（完成） | `Exercise::to_player()` 剥离全部答案字段（`correct`/`answer`/`references`/`expected`，`word-match` 的 `pairs` 拆为 `lefts`/`rights`）；`training_start` 与 `assessment_modules` 下发 player 形态，快照/仓储保留完整题目；页面从 `AnswerCheck.correctAnswer` 取"正确答案/参考表达/正确配对" | Rust 测试 4 项（各题型无答案字段、模块无答案字段、服务端判分、越权拒绝） |
| 2.2.2 判分权收归（第一步） | 新增 `training_check` / `assessment_check` + `services/answer_service.rs`；`AnswerCheck{correct, partialScore, correctAnswer, explanation}`；只能针对本会话/本模块的题目发问；训练与测评作答控件改为向服务端求证（判分不再发生在页面） | `cargo test answer_service` 3 项；`tests/integration/answer-check.test.ts` 2 项（对/错/题目不存在） |
| 2.2.4 学习态归一 | `Vocabulary` / `GrammarPoint` / `Sentence` 删除 `mastery/state/reviewCount/dueAt/strength/lastReviewedAt`；新增 `EntityStateSeed` 与 `mock/state-hints.ts` 承载**首次建库**的初始值；`KnowledgeDetail` 增加独立 `state` 字段；详情页从 `detail.state` 读取 | `tests/integration/knowledge-state.test.ts` 3 项：内容实体无学习态字段、详情 `state` 与 `entityStates` 同值、列表摘要与状态一致 |

### 16.2 测试基线（2.2 中途）

- `cargo test`：**80 + 5** 通过（原 75 + 5）
- 新增 Rust 测试：`answer_service` 3 项（服务端按键判分、拒绝会话外题目、拒绝跨模块提问）
- `npm test`：**85** 通过（原 73）
- `npm run typecheck` / `npm run lint` / `npm run check:content`：全部通过

### 16.3 未完成（下次继续）

| 任务 | 为什么押后 | 建议做法 |
|---|---|---|
| 2.2.2 判分权收归 | 需要拆 `ExerciseForPlayer` / `ExerciseWithKey`，并改变"答题后立即看答案"的反馈时序——属于**交互变化**，必须单独评审，不能夹带在模型对齐里 | 先只改后端下发结构（新增 `exercise_for_player` 视图），UI 暂时仍走完整题目；再单独提交 UI 改动 |
| 2.2.4 学习态单一事实源 | 要同时改 `mock/{vocabulary,grammar,sentences}.ts`、`seed.ts`、`knowledge.service.ts`、`selectors.ts`、`types/*`，影响面最大 | ✅ 已完成 |
| `shadowing` 的答案隐藏 | 该题型 `text` 既是朗读素材又是判分目标，无音频就无法隐藏 | 需要 TTS 或音频资源后再处理；当前在 `API_CONTRACTS.md` 标注为已知限制 |

### 16.4 与 2.1 文档的差异（已回写）

- `BUSINESS_MODEL_REVIEW.md` N18、`DATABASE_DESIGN.md` G4 / §11：**"`training_options.kind` 应加 UNIQUE"的结论已被证伪并更正**（`multiple-choice` 跨 track 重复出现），真正修复在查询层。
