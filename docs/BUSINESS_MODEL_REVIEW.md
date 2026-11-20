# 业务模型审查报告（第二阶段 · 2.1）

> 审查方式：逐文件读取 `src/types`、`src/services`、`src/stores`、`src/lib`、`src/features`、`src-tauri/src`、`src-tauri/src/database/migrations/0001_init.sql`、`content_pack.json`、`Cargo.toml`、`tauri.conf.json`、`capabilities/default.json`、`package.json` 与既有 docs。
> 本文只记录**事实与结论**，不修改任何业务代码、不建库、不迁移。
> 与既有 `docs/DATA_MODEL_REVIEW.md` 的关系：那份文档（阶段 A）确立缺陷编号 `D1–D14` 并驱动了迁移 `0001_init.sql`；本文是在迁移落地后的**复核**，沿用 `D*` 编号，新增问题使用 `N*` 编号。两文冲突时以本文为准。

---

## 0. 结论摘要

1. **数据层地基已经落地，且质量高于预期**：`0001_init.sql` 已建 35 张业务表 + 1 视图，WAL、`foreign_keys=ON`、`busy_timeout`、迁移校验和、迁移前备份、内容包幂等同步都已实现并有测试（`cargo test` 80 项全绿）。
2. **真正的风险不在表结构，而在"两套运行时"并存**：前端 `mock` 分支仍在用 localStorage 整份 `LearningDb` 快照 + 自己判分 + 自己算 SRS；Rust 分支已有 SQLite 权威存储。两条路径的语义（判分权、幂等键、掌握度写入）**不一致**，一旦切换数据源就会出现"分数对不上"。
3. **功能缺口集中在 4 个命令面**：`dashboard_snapshot`、`analytics_query`、`plan_*`（4 条）、`review_*`（2 条，未注册）。共 8 条命令在 Rust 侧未实现，其中 6 条恒返回 `unavailable`。因此在 Tauri 运行时用 `backend` 数据源，**首页 / 分析 / 规划 / 复习 4 个页面会直接报错**。
4. **`review_logs` 是"有表、有索引、有 XP 公式引用、但没有写入路径"的悬空表**：复习这条链路是数据模型里唯一的完全空洞，且它同时是掌握度与 XP 的第二来源。
5. **重复数据问题已从"前端重复"转移到"数据库内重复"**：`kind` 三处冗余、`score/accuracy/xp/kind_scores` 可重算却落库、`entities` 与 `entity_states` 学习态双份、`plan_items.done` 与 `status` 双标志。
6. **`sec_speak_eval` 空分项目前是**设计内语义**，数据侧、UI 侧、判分侧三处都做了正确处理**，不构成缺陷（详见 §4.1.4）。

---

## 1. 审查范围与真实分层

### 1.1 分层事实

```
React 页面 (src/features/*)
  ↓ useQuery / hooks
services/api/*.service.ts      ← 业务编排在这里（判分、写库、重排计划）
  ↓ serve()  (src/services/api/transport.ts:113)
  ├─ mock  : services/data/*  + services/mock/*  → localStorage「sage.learning.db」
  └─ backend: @tauri-apps/api/core invoke → src-tauri #[tauri::command] → SQLite
```

- 数据源开关：`VITE_SAGE_DATA_SOURCE`（默认 `mock`），且 `serve()` 的条件是 `dataSource === "mock" || !isTauriRuntime()`（`src/services/api/transport.ts:4-12, 113`）。**这不是"按命令能力"判定，而是"是否在 Tauri 里"判定**，是本阶段最大的架构性风险之一（见 N2）。
- Rust 侧：`Database { conn: Mutex<Connection>, path }`（`src-tauri/src/database/mod.rs:34-37`），单连接、无连接池、WAL 常开。命令层统一 `async + spawn_blocking`（`src-tauri/src/commands/mod.rs:22-31`），不阻塞 UI 线程的设计已落实。
- 学习态单一入口：`services/state_updater::apply_performance`（`src-tauri/src/services/state_updater.rs:14-65`），训练与测评都走它，SRS 与掌握度算法在 Rust 侧各只有一份。**这一点是当前模型最健康的地方，必须保留。**

### 1.2 已实现的命令面（24 条）

| 域 | 命令 | 状态 |
|---|---|---|
| 应用 | `app_health`、`app_info` | 可用 |
| 用户/设置 | `settings_get/update`、`user_profile_get/update`、`sync_status_get`、`local_data_summary` | 可用（真实读写 SQLite） |
| 用户/设置 | `sync_trigger`、`local_data_clear` | 恒 `unavailable` |
| 总览 | `dashboard_snapshot` | 恒 `unavailable` |
| 分析 | `analytics_query` | 恒 `unavailable` |
| 计划 | `plan_get`、`plan_regenerate`、`plan_apply_difficulty`、`plan_toggle_item` | 恒 `unavailable` |
| 测评 | `assessment_modules`、`assessment_submit` | 可用 |
| 训练 | `training_options`、`training_start`、`training_submit` | 可用 |
| 知识库 | `knowledge_graph`、`knowledge_list`、`knowledge_detail`、`knowledge_neighbors` | 可用 |
| 复习 | `review_queue`、`review_submit` | **前端契约已声明（`contracts.ts:46-48`），Rust 侧未注册** |

证据：`src-tauri/src/lib.rs:24-57`（注册表）、`src-tauri/src/commands/learning.rs:11-46`（6 条 `unavailable`）。

---

## 2. 跨模块重复数据矩阵

"同一事实被几个地方各自维护"是本项目最主要的技术债。以下矩阵只列**真实在写入**的位置。

| 事实 | 维护点 1 | 维护点 2 | 维护点 3 | 结论 |
|---|---|---|---|---|
| 掌握度 / 调度状态 | `entity_states`（Rust 权威） | `Vocabulary/GrammarPoint/Sentence` 上的 `mastery/state/dueAt/...`（`src/types/vocabulary.ts:48-58`、`grammar.ts:51-55`、`sentence.ts:31-35`） | `db.entityStates`（localStorage 快照） | **三份**。内容实体上的学习态字段永远陈旧（写入只更新 `entityStates`），靠 `knowledge.service.ts:31-69` 覆盖掩盖 |
| `kind`（实体类型） | `entities.kind` | `entity_states.kind`（upsert 时无条件覆盖，`state_repository.rs:44`） | `mastery_events.kind`（快照写入，`training_service.rs:283`） | **三份**，可漂移 |
| 训练得分 | `training_sessions.score/max_score/accuracy/xp` | `training_answers.partial_score` 可重算 | `training_kind_scores` 可聚合得到 | 派生值落库 ×3 |
| 测评总分 | `assessment_attempts.score` | `assessment_section_results.score` 平均值可重算 | `accuracy` 直接等于 `score`（`assessment_service.rs:152`） | 派生值落库，且 `accuracy` 与 `score` 毫无语义差异 |
| 计划完成率 | `plan_items.done` | `plan_items.status` | 服务端不存，前端 6 处现算（`learning-db.ts:375/397`、`planner.ts:164`、`seed.ts:57`、`planner-page.tsx:53`、`mock/planner.ts:236`） | **双标志 + 6 处重算** |
| 科目/分项得分 | `assessment_section_results` | `AssessmentAttempt.perSection`（前端内联结构） | `SectionOutcome`（Rust） | 三个同构定义 |
| 复习记录 | `review_logs`（**零写入**） | `db.reviewLogs`（localStorage） | `mock/learning.ts` 硬编码 | 表空着，前端在别处记账 |
| 学习计划 | `study_plans`（仅被 `SELECT difficulty`） | `plan_days/plan_items/plan_weekly_goals/plan_advice`（**零读写**） | 前端 `db.plan`（unavailable 时的唯一真相） | 表结构预留但未接线 |
| 任务 | `DashboardTask`（`types/dashboard.ts:16-27`） | `PlanItem`（`types/plan.ts:55-68`） | `study_plans` 子表 | 字段近乎全同，两份映射代码 |
| 能力画像 | `StudyPlan.profile`（落库） | 每次请求 `decoratePlan` 重算（`selectors.ts:271-283`） | Rust `ability::build_ability_profile`（**无调用者**） | 落库的画像与重算的画像会打架 |

---

## 3. 数据不一致风险（按后果排序）

| # | 风险 | 触发条件 | 后果 | 证据 |
|---|---|---|---|---|
| R1 | 客户端判分被篡改 | `mock` 数据源，或前端把 `correct/partialScore` 直接写库 | 分数、掌握度、计划全部失真 | `types/exercise.ts:28,72-120`、`training-session.tsx:53`、`assessment.service.ts:78`、`learning-db.ts:185` |
| R2 | 历史成绩被覆盖 | 计划整份重建 / 上传同名 submission | 旧成绩消失、趋势图断裂 | `assessment.service.ts:163`、`learning-db.ts:148` |
| R3 | 掌握度跨页不一致 | 内容实体上的旧 `mastery` 被直接渲染 | 知识库显示 0% 而首页显示 65% | `entity_repository.rs:240-246/323-327/367-371`（仓储硬编码 `mastery:0.0`），靠 service 覆盖 |
| R4 | 复习与训练无法关联 | `review_logs` 无写入、无 `ref_id` 到训练事件 | 无法回答"这次复习是针对哪次训练暴露的弱点" | `0001_init.sql:333-350` |
| R5 | 删除知识点后历史失联 | 5 张事件表的外键缺失 | 历史行指向不存在的题/分项 | `0001_init.sql:272/291/314/325/359` |
| R6 | 统计口径漂移 | 指标在 6 处各算一遍 | 同一份数据在不同页面显示不同正确率 | `stats.ts:134/160/203`、`selectors.ts:131`、`analytics-page.tsx:25` |
| R7 | 时间双轨 | ISO 文本入库、毫秒整数跨层 | 跨时区/跨零点归日错误 | `database/mod.rs:30`、`training_repository.rs:107-108`、`stats.ts:66` |
| R8 | 修订号语义失真 | 只读补种也 `bump_revision` | 前端"数据变了"误报，缓存反复失效 | `database/mod.rs:103-106` |
| R9 | 首次迁移丢 localStorage | 切 backend 且不做导入 | 用户历史学习记录归零 | `repository.ts:38`（schemaVersion 不符直接重种子化） |
| R10 | 打包态功能不可用 | 在 Tauri 中以非 mock 数据源启动 | 首页/分析/规划/复习直接报 `unavailable` | `transport.ts:113` + `commands/learning.rs:11-46` |

---

## 4. 逐模块审查

### 4.1 Assessment（测评）

**现有实体**：`assessment_modules` → `assessment_sections` → `assessment_section_exercises` → `exercises`；作答落 `assessment_attempts` + `assessment_section_results` + `assessment_answers`；掌握度变化写 `mastery_events(source=assessment)`。

**写入路径（Rust，已接线）**
1. `assessment_service::submit` 单事务：幂等检查 → 读模块 → 逐分项判分 → 汇总 → 写 attempt/section_results/answers → 逐实体 `apply_performance` → 写 `mastery_events`（`assessment_service.rs:25-226`）。
2. 总分口径：**只对有题分项的分数取平均**（`assessment_service.rs:98-104`），空分项不参与，不产生 NaN。
3. 等级：`ability::level_of_score(score)`（`:139`）。
4. `strengths/weaknesses/recommendation` 作为**快照**落库（`:145-160`）——正确，因为它们依赖当时的排名。
5. `follow_up` **不落库**，每次读回重算（`assessment_repository.rs:245-247`）——不一致，见 N6。

**判分**：Rust 侧只信库内 payload，忽略客户端 `correct/partialScore`（`training_service.rs:139-155` 同构逻辑，测评同理）。前端 `mock` 分支则在客户端 `scoreAnswer` 后写库（`assessment.service.ts:74-91`）。

**4.1.4 空分项 `sec_speak_eval` 的业务语义（明确结论）**

| 层面 | 事实 | 是否异常 |
|---|---|---|
| 内容包 | `sec_speak_eval` 标题"口语能力评估"，描述"汇总以上四项，给出分级与改进建议"，`sort_order=3`，不挂题 | 否，设计为汇总项 |
| 库表 | `assessment_section_exercises` 中无该分项的行；引用完整性检查 `missing 0` | 否 |
| 判分 | `total == 0` → `score = 0`，且汇总时被 `filter(total > 0)` 排除（`assessment_service.rs:89-104`） | 否，已正确规避 |
| UI | 显示"评估汇总"而非"0 题"（`src/features/assessment/assessment-page.tsx:192-194`） | 否，已正确处理 |
| 测试 | 断言已放宽为"每个模块题目总数 > 0"（`assessment_service.rs:292-296`） | 否，已对齐语义 |

→ **结论：空分项是合法的一等公民，不能被当作脏数据清理，也不应通过补题来"修复"。模型层应显式区分 `section.kind` 上的"可作答分项"与"汇总分项"，本期建议在 `assessment_sections` 增加一列语义标记（见 `LEARNING_DATA_MODEL.md` §6.3）。**

**问题**
- N1（P1）前端 `applyAssessmentSubmit` 把测评产生的掌握度事件记成 `source: "training"`（`learning-db.ts:250`），而类型只允许 `training|review`（`learning-state.ts:100`）→ 趋势回放无法区分测评与训练。Rust 侧已正确写 `source="assessment"`（`assessment_service.rs:205`），**两端不一致**。
- N6（P1）`AssessmentOutcome` 一半字段持久化、一半重算，同一响应对象的字段可信度不同。
- N7（P2）不传 `submissionId` 时幂等键退化为 `{module_id}_{now}`（`assessment_service.rs:31-33`），重复提交会重复记账；前端页面其实已经生成 `crypto.randomUUID()`（`assessment-page.tsx:143`），只是没强制。

### 4.2 Training（训练）

**现有实体**：`training_options`（入口）→ `exercises`（题库）+ `exercise_entities`（题→知识点）；会话快照 `training_session_snapshots`；事实 `training_sessions` + `training_answers` + `training_kind_scores` + `training_session_entities`。

**写入路径**：`training_service::submit` 单事务 7 步，包含判分、XP、四张表写入、逐实体掌握度、快照 `consumed_at`（`training_service.rs:113-319`）。

**优点**
- 服务端快照判分（Q2 已确认的方案 B），客户端只传 `{sessionId, answers}`；`tests/parity.rs` 保证 TS/Rust 判分一致。
- XP 有独立算法 `algorithms/xp.rs`，等级曲线 `level_from_xp` 已存在。

**问题**
- N3（P1）`training_sessions.session_id` 与 `submission_id` 训练路径恒等（`training_service.rs:207-208`），幂等又有两条机制（`submission_id` UNIQUE + 快照 `consumed_at`）。
- N4（P1）`training_sessions.difficulty` 用**第一题**的难度覆盖快照难度（`:193-198`），统计口径错。
- N5（P2）同一实体在一次会话中被多次 `upsert` + 多次写 `mastery_events`（`:254-291`），掌握度被"同一批题反复学习"过度加权。
- N8（P2）`kind_scores` 完全可由 `answers JOIN exercises` 聚合，落库属冗余。
- N9（P3）XP 总数 `total_xp` 与等级 `level_from_xp` **无生产调用者**，`dashboard_snapshot` 又是 `unavailable` → 前端首页的 XP/等级只能来自 mock。

### 4.3 Review（复习）

**现状：这是唯一一条"数据模型存在但业务未落地"的主链路。**

- Rust：`review_logs` 表 + 3 个索引已建（含 `ux_review_submission` 部分唯一索引），但**全库无任何 INSERT**，仅被 `local_data_summary` 计数（`user_repository.rs:186`）与 `xp.rs:3` 的注释引用。**没有** `review_queue` / `review_submit` 命令。
- 前端（mock 路径）：队列来自 `entityStates.filter(isDue)`（`selectors.ts:85-93`），下次复习时间由 **TS 版 SRS** 计算（`learning-db.ts:288-297` → `lib/algorithms/srs.ts:68-110`），一次提交写 4 处（`entityStates`、`reviewLogs`、`masteryEvents`、计划勾选）。
- 语义缺口：`dueAt === null` 的实体永不出队（`srs.ts:113`）→ "新词"不进复习；复习提交**无幂等键**；`ReviewRecord.id = "rv_" + entityId`（`selectors.ts:73`）不是记录 id。

**结论**：复习与掌握度的关系**在 Rust 侧尚未建立**，"复习记录 → 掌握度 → 调度"的闭环只存在于前端 TS。2.3 阶段必须把它做成与训练同构的服务端事务（见实施计划 2.3）。

### 4.4 Planner（学习计划）

- 计划来源：**本地确定性规则引擎**，非 AI（`lib/algorithms/planner.ts:70-172`，`model: "sage-planner-v1 (rule-based)"`）。首次安装来自种子 `mock/planner.ts:141-245`。
- 输入：`ability`、`dueCount`、`recentAccuracy(最近10次)`、`weeklyMinutes/activeDays/newMastered/grammarCorrect`（`learning-db.ts:445-511`）——**全部来自学习事件与状态，口径正确**。
- 输出：`PlanDay[]`，但 `week[1..6]` 的 `items: []`（`planner.ts:315-325`），**只有今天有真实任务**。
- 完成状态：手动翻转 + 训练/复习按 `kind` 自动勾选 + 测评后整份重排（`assessment.service.ts:163`，会覆盖当天手动勾选）。
- 库表：`study_plans` 只被 `training_service` 读 `difficulty`（`training_service.rs:100-107`）；`plan_days/plan_items/plan_weekly_goals/plan_advice` **零读写**。

**问题**
- N10（P1）计划是"前端唯一真相"，Rust 侧 4 条 `plan_*` 命令全 `unavailable`，库表空转。
- N11（P2）完成率 6 处重算、`done`/`status` 双标志、重排时"同 kind 误继承"（`planner.ts:329-341`）。
- N12（P2）口语项插入分支条件写错（`planner.ts:113` 与 `:130` 的 `if/else if` 绑定），预算充裕时口语任务永不生成。
- N13（P3）`planMeta` 与 `plan.generatedAt` 双写。
- N14（P3）"未来 6 天是占位空数组"，前端无法区分"未生成"与"0 任务"。

### 4.5 Knowledge（知识库）

- 内容：`entities` + 6 张明细表 + `knowledge_relations` + 视图 `knowledge_index`；内容包幂等同步（`content.rs:153-189`）。
- 学习态：`entity_states`，由 `knowledge_service::states()` 一次性 `load_all` 后内存过滤（`knowledge_service.rs:21-74`）。
- 图谱布局：Rust `algorithms/layout.rs` 计算坐标与权重（`knowledge_service.rs:84-136`）。

**问题**
- N15（P2）`knowledge_list` 全表载入 + 内存过滤，**无分页**；`KnowledgeFilter` 靠手写 `from_json`。
- N16（P2）三层 N+1：module→section→exercise（`assessment_repository.rs:71-84`）、pool 逐题查关联（`exercise_repository.rs:38-48`）、detail 逐 id 查内容（`knowledge_service.rs:215-220`）。
- N17（P2）内容仓储在模型里硬编码假学习态（`mastery: 0.0, state: "new"`），一旦有人绕过 service 直出就会显示 0%。
- N18（P3，**已修复**）`option_item_count` 只按 `kind` 单条件 `query_row`，同 kind 多行时结果不确定（`exercise_repository.rs:138-145`）。
  **修订**：审查初判"kind 应加 UNIQUE"经数据核对被证伪 —— `multiple-choice` 同时存在于 vocabulary 与 grammar 两个 track（12 入口 / 11 种 kind），加唯一约束会让内容包无法同步。真正的问题是查询条件不完整，2.2 已改为 `(kind, track, sort_order)` 三要素查询并补 `idx_training_options_kind_track` 索引。
- N19（P3）内容同步只按声明的冲突键 UPSERT，`ux_vocabulary_term/ux_grammar_title/ux_sentences_en` 未纳入 → "改错别字"式内容更新会 constraint violation；列名由 JSON key 直拼 SQL，多字段即运行期报错。

### 4.6 Analytics（分析）

- 前端 `buildAnalyticsData(db, range, now)` 遍历 `trainingAttempts + reviewLogs + entityStates + masteryEvents` 现算（`selectors.ts:228-268`），**口径本身是正确的"从统一学习记录派生"**。
- 但存在两套并存：
  1. 真实派生（`stats.ts`）；
  2. **硬编码假数据**仍在 barrel 导出：`mock/learning.ts:28-62/106-117/162-184/191-221`、`mock/analytics.ts:32-51`、`mock/dashboard.ts:27-39`。
- 种子历史是**编造的**：`buildSeedAttempts` 把 mock 记录转成 12+ 条 attempt，答案 `correct: i < record.correct` 人工构造（`seed.ts:139-172`）；`buildSeedMasteryEvents` 把"当前掌握度平摊成 N 次提升"（`seed.ts:206-235`）。
- 复习时长按 `REVIEW_MINUTES_PER_CARD = 0.5` 估算（`stats.ts:29`），非实测。

**结论（回答"Analytics 能否基于统一学习记录计算"）**：**能，且架构方向正确**。前提是三件事：
1. 删掉硬编码假数据，只留"种子输入"；
2. 指标计算收敛到**一个实现**（建议 Rust 侧 `analytics` 服务，前端只渲染）；
3. `review_logs` 必须真正开始写入，否则"复习完成量/复习时长"永远为 0 或靠估算。

---

## 5. 后端契约缺口清单

| # | 缺口 | 影响 | 处理阶段 |
|---|---|---|---|
| C1 | `review_queue` / `review_submit` 未注册 | 复习页在 backend 模式不可用 | 2.3 |
| C2 | `dashboard_snapshot` 恒 `unavailable` | 首页 XP/等级/最近记录/复习队列不可用 | 2.5 |
| C3 | `analytics_query` 恒 `unavailable` | 分析页不可用 | 2.5 |
| C4 | `plan_get/regenerate/apply_difficulty/toggle_item` 恒 `unavailable` | 计划页不可用 | 2.4 |
| C5 | `sync_trigger` / `local_data_clear` 恒 `unavailable` | 设置页"同步/清空"按钮假可用 | 2.4 / 2.6 |
| C6 | `transport.ts` 按"是否 Tauri"切源，而非按命令能力 | 任一条命令缺失即整页报错 | 2.2（契约层） |
| C7 | `Page<T>` 已定义但无接口使用 | 列表无分页契约 | 2.2 |
| C8 | 幂等键不统一（训练=sessionId、测评=可选、复习=无） | 重复提交行为不一致 | 2.2（模型层） |
| C9 | `error.rs` 的 `database_too_new` 借用 `unavailable` 码 | 前端提示"服务暂不可用"，实际需升级 | 2.2 |

---

## 6. 问题总表（按优先级）

### P0 — 数据正确性 / 契约断裂

| 编号 | 问题 | 证据 |
|---|---|---|
| N2 | 数据源切换按运行时判定，8 条命令缺失导致 4 个页面在打包态不可用 | `transport.ts:113`、`commands/learning.rs:11-46` |
| R1/N20 | 判分权与正确答案在客户端（`correct`、`answer`、`expected` 随题下发，4 处前端判分） | `types/exercise.ts:28,72-120,43`、`training-session.tsx:53`、`assessment.service.ts:78` |
| N21 | `review_logs` 零写入却参与 XP 公式与本地数据统计 | `0001_init.sql:333`、`xp.rs:3`、`user_repository.rs:186` |

### P1 — 静默错数据 / 无法追溯

| 编号 | 问题 | 证据 |
|---|---|---|
| N1 | 测评掌握度事件被记为 `training` | `learning-db.ts:250` |
| N3 | `session_id == submission_id` + `consumed_at` 双幂等 | `training_service.rs:127-129,207-208` |
| N4 | 会话难度取首题而非快照 | `training_service.rs:193-198` |
| N4b | `lapse_count` 双算（`mastery` 的结果被静默丢弃，最终取 `srs` 的） | `mastery.rs:129`、`srs.rs:120`、`state_updater.rs:54` |
| N6 | `followUp` 不落库，同响应字段可信度不一 | `assessment_repository.rs:245-247` |
| R5 | 5 处历史表外键缺失，内容变更会打断历史 | `0001_init.sql:272,291,314,325,359` |
| R3/N17 | 学习态双份 + 仓储硬编码假掌握度 | `entity_repository.rs:240-246` |
| R6 | 指标 6 处各算 | `stats.ts:134/160/203`、`planner-page.tsx:48-58` |
| N10 | 计划仅存于前端，库表空转 | `commands/learning.rs:26-46` |
| R7 | 时间双轨（ISO 文本 / 毫秒整数 / 本地日） | `database/mod.rs:30`、`stats.ts:66` |

### P2 — 设计与规模

| 编号 | 问题 | 证据 |
|---|---|---|
| N5 | 同实体一会话多次加权 | `training_service.rs:254-291` |
| N8 | `kind_scores` 可聚合却落库 | `0001_init.sql:281-287` |
| N11/N12/N13/N14 | 计划完成率、双标志、分支 bug、周内占位 | `planner.ts:113,130,315-341` |
| N15/N16 | 全表扫描 + 三层 N+1，无分页 | `knowledge_service.rs:21-74,215-220` |
| N18/N19 | 内容同步约束与列白名单缺口 | `content.rs:90-130` |
| R8 | 只读补种也递增 revision | `database/mod.rs:103-106` |
| N22 | `entity_states.row_version` 写而不读 | `0001_init.sql:225`、`models/state.rs:10-27` |
| N23 | 死代码/未接线：`backup::restore`、`health_check`、`reset_all`、`total_xp`、`level_from_xp`、`assessment_summaries` | 见各文件 |

---

## 7. 值得保留的设计（不要在后续阶段改掉）

1. 内容 / 学习态 / 事件 / 派生视图 **四层分家**，且 `0001_init.sql` 已按此建表。
2. 学习态唯一写入口 `state_updater::apply_performance`。
3. 训练会话快照 `training_session_snapshots` + 服务端判分（防作弊、可复算）。
4. 事件表只追加（无 `updated_at`），趋势由事件回放而非当前值倒推。
5. `submission_id` + 唯一索引做幂等，而不是内存 `find`。
6. 迁移前的强制备份 + 校验和 + `database_too_new` 拒绝启动。
7. 前端 `services/api` 层保持 DTO 形状不变，页面层完全不用改。

---

## 8. 需要你决定的问题（本阶段不擅自决定）

| # | 议题 | 选项 A | 选项 B | 建议 |
|---|---|---|---|---|
| Q1 | 判分权 | 全部收归 Rust，前端只提交 `{exerciseId, value, elapsedMs}` | 保留前端判分（离线可用） | **A**。B 会导致 mock 与 backend 分数不可比，且可篡改 |
| Q2 | 正确答案是否下发 | 拆 `ExerciseForPlayer` / `ExerciseWithKey`，答案只在服务端 | 继续随题下发（UI 需要即时讲解） | **A**，讲解改为提交后由服务端回传 |
| Q3 | 派生列去留 | 删除 `accuracy/kind_scores/score` 等，改视图 | 保留冗余列，写入时保证一致 | **A**（`kind_scores` 可留作性能优化，但必须有唯一写入点） |
| Q4 | 计划生成位置 | 前端规则引擎保留，只把结果持久化到 Rust | 规则引擎整体搬到 Rust | **A**：本期先持久化，避免二次移植风险 |
| Q5 | localStorage 历史 | 一次性导入 SQLite | 声明"2.x 从种子开始" | **A**（用户历史不可丢） |
| Q6 | `review` 与 `training` 是否共用一张事件表 | 保持两表，靠 `mastery_events` 聚合 | 合并为 `learning_records` 宽表 | **A**：宽表会再次引入"派生列"问题 |

---

## 9. 移交下一份文档

- 实体定义与关系 → `LEARNING_DATA_MODEL.md`
- 表结构/索引/约束/事务/备份 → `DATABASE_DESIGN.md`
- 分层与线程模型 → `RUST_DATABASE_ARCHITECTURE.md`
- 构建环境验证 → `TAURI_BUILD_READINESS.md`
- 2.2–2.6 任务拆分与验收 → `PHASE_2_IMPLEMENTATION_PLAN.md`

> 本次审查仅读取文件，未修改任何业务代码、未创建数据库、未执行迁移。
