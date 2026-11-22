# API 契约

本文件是 React ⇄ Rust 的唯一契约清单。命令名常量定义在
`src/services/api/contracts.ts`，Rust 侧实现位于 `src-tauri/src/commands/*.rs`。

---

## 1. 传输层协议

### 返回信封

前端所有服务函数返回：

```ts
type ApiResult<T> = { ok: true; data: T } | { ok: false; error: ApiError };

interface ApiError {
  code: "unavailable" | "not_found" | "invalid_argument" | "unauthorized"
      | "rate_limited" | "timeout" | "internal";
  message: string;   // 面向用户：出了什么问题
  hint?: string;     // 面向用户：怎么修复
  detail?: string;   // 面向开发者：技术细节
  retryable: boolean;
}
```

页面与 hooks 只消费 `ApiResult`，不感知数据来源。

### 错误序列化约定

Rust 命令返回 `Result<T, AppError>`，`AppError` 通过自定义 `Serialize` 输出字符串：

```
"not_found:找不到 w_unknown"
```

`transport.normalizeError` 按第一个 `:` 切分，映射为 `ApiError`，并把 `code` 翻译成中文
用户文案（映射表见 `transport.toUserMessage`）。**Rust 不要返回嵌套对象错误**，
否则前端只能归类为 `internal`。

### 数据源切换

```
.env / .env.local
VITE_SAGE_DATA_SOURCE=mock      # 默认：本地模拟数据
VITE_SAGE_DATA_SOURCE=backend   # 通过 Tauri invoke 调用 Rust
```

判断逻辑在 `transport.serve()`：当 `dataSource === "backend"` 且运行在 Tauri 内
（`window.__TAURI_INTERNALS__` 存在）时走 `invoke`，否则回落到 mock。

### 取消与超时

`useQuery` 通过 `AbortSignal` 取消请求。Mock 层在延迟中监听 `abort` 并抛出
`DOMException("Aborted")`；前端忽略该异常，不产生错误 UI。Rust 侧当前不实现取消，
第二阶段可用请求 ID + 取消命令补齐。

---

## 2. 命令清单

参数名在前端为 camelCase，Tauri 会自动映射到 Rust 的 snake_case；下表「入参」按前端视角书写。
返回结构由 Rust 侧的 `#[serde(rename_all = "camelCase")]` 保证与 `src/types/*.ts` 完全一致。

> 复习闭环（`review_queue` / `review_submit`）已在 2.3 接入 SQLite，
> `contracts.ts` 与 Rust 侧状态一致（`ready`）。

### 应用

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `app_health` | — | `boolean` | ✅ 已实现 |
| `app_info` | — | `{ name, version, stage, dataSource }` | ✅ 已实现 |

### 用户与设置

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `settings_get` | — | `AppSettings` | ✅ **SQLite** |
| `settings_update` | `patch: Partial<AppSettings>` | `AppSettings` | ✅ **SQLite**（点分键 UPSERT，语义等同深合并） |
| `user_profile_get` | — | `UserProfile` | ✅ **SQLite** |
| `user_profile_update` | `patch: Partial<UserProfile>` | `UserProfile` | ✅ **SQLite**（局部更新；资料变更后重排计划待阶段 C5） |
| `sync_status_get` | — | `SyncStatus` | ✅ **SQLite**（如实返回"仅本机" + 真实占用） |
| `sync_trigger` | — | `SyncStatus` | ⛔ 明确不可用（云同步不在第二阶段范围） |
| `local_data_summary` | — | `LocalDataSummary` | ✅ **SQLite**（真实计数 + 文件大小） |
| `local_data_clear` | — | `{ cleared: true }` | ⏳ 阶段 C7（需与计划重建同事务） |

### 学习总览与数据

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `dashboard_snapshot` | — | `DashboardData` | ⏳ 占位 |
| `analytics_query` | `range: 7｜14｜30` | `AnalyticsData` | ⏳ 占位 |

### AI 规划

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `plan_get` | — | `StudyPlan` | ⏳ 占位 |
| `plan_regenerate` | — | `StudyPlan` | ⏳ 占位 |
| `plan_apply_difficulty` | `level: number` | `StudyPlan` | ⏳ 占位 |
| `plan_toggle_item` | `itemId: string` | `PlanItem[]` | ⏳ 占位 |

### AI（SAGE Intelligence，1.0.1 新增）

完整契约见 [AI_INTEGRATION.md](./AI_INTEGRATION.md)。

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `ai_status` | — | `AiStatus` | ✅ **SQLite + Library** |
| `ai_version` | — | `AiVersionInfo` | ✅（版本来自 Cargo.lock，编译期注入） |
| `ai_provider_upsert` | `provider: AiProviderInput` | `AiProviderView` | ✅ **SQLite**（密钥写入独立文件） |
| `ai_provider_delete` | `providerId` | `boolean` | ✅ **SQLite + 文件** |
| `ai_provider_test` | `providerId?` | `AiProbeResult` | ✅（一次极小真实请求探测） |
| `ai_complete` | `prompt, providerId?, model?, system?` | `AiCompletionResult` | ✅ |
| `ai_stream_start` | 同上 | `AiStreamHandle` | ✅（事件流） |
| `ai_stream_cancel` | `requestId` | `boolean` | ✅（协作式取消） |

事件：`ai-core://started｜text-delta｜reasoning-delta｜tool-call-*｜structured-output｜usage-updated｜error｜completed`，
以及客户端终态事件 `sage-ai://stream-finished`。
非 Tauri 环境（浏览器 / mock）下这些命令统一返回 `unavailable`，不伪造结果。

### 测评

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `assessment_modules` | — | `AssessmentModule[]` | ✅ **SQLite** |
| `assessment_submit` | `moduleId, answers: ExerciseAnswer[], submissionId: string`（必填） | `AssessmentOutcome`（含 `duplicate`） | ✅ **SQLite** |
| `assessment_check` | `moduleId, exerciseId, value: Json` | `AnswerCheck` | ✅ **SQLite**（2.2 新增：逐题即时反馈） |

### 训练

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `training_options` | `track?: TrainingTrack` | `TrainingOption[]` | ✅ **SQLite**（`itemCount` 按题库容量收敛） |
| `training_start` | `kind, track, count?, title?, difficulty?` | `TrainingSession` | ✅ **SQLite**（题目快照落库） |
| `training_submit` | `sessionId, answers` | `ExerciseResult`（含 `duplicate`） | ✅ **SQLite**（服务端按快照判分，忽略客户端 `correct/partialScore`） |
| `training_check` | `sessionId, exerciseId, value: Json` | `AnswerCheck` | ✅ **SQLite**（2.2 新增：逐题即时反馈） |

> `training_submit` 已按 Q2 方案 B 实现：前端只传 `sessionId + answers`，
> 题目与正确答案以 `training_session_snapshots` 为准，判分不可被前端篡改。
>
> `AnswerCheck = { exerciseId, correct, partialScore, correctAnswer, explanation }`。
> 只允许对本会话/本模块内的题目发问（防"拿题库当答案查询器"）。
>
> **答案已从下发 DTO 中移除**（`Exercise::to_player`）：
> - `multiple-choice` / `cloze`：去掉 `options[].correct`（cloze 另去 `answer`）
> - `spelling` / `dictation` / `translate` / `rewrite` / `error-correction` / `sentence-order`：去掉 `answer`、`references`
> - `word-match`：`pairs` 拆成 `lefts` / `rights`（正确配对只在 `AnswerCheck.correctAnswer` 里出现）
> - `roleplay`：去掉 `turns[].expected`
> - 快照（`training_session_snapshots`）与仓储仍保留完整题目，判分与校验从库里读。
>
> **已知限制**：`shadowing` 的 `text` 既是朗读素材又是判分目标，无法隐藏。

### 知识库

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `knowledge_graph` | — | `KnowledgeGraph` | ✅ **SQLite**（坐标由 `algorithms/layout` 现算） |
| `knowledge_list` | `filter: KnowledgeFilter` | `KnowledgeSummary[]` | ✅ **SQLite** |
| `knowledge_detail` | `id: string` | `KnowledgeDetail`（可辨识联合） | ✅ **SQLite** |
| `knowledge_neighbors` | `id: string` | `KnowledgeSummary[]` | ✅ **SQLite** |

> 「占位」= 命令已注册可调用，返回 `unavailable` 错误。这样前端切到 `backend`
> 时能得到明确错误而不是白屏，也让第二阶段可以逐条替换、逐个验证。

### 复习

| 命令 | 入参 | 返回 | 状态 |
|---|---|---|---|
| `review_queue` | `limit?: number`（默认 20，上限 200） | `{ dueCount, cards: ReviewCard[] }` | ✅ **SQLite**（2.3 新增：内容 + 调度预览现算） |
| `review_submit` | `entityId, rating: "again"\|"hard"\|"good"\|"easy", submissionId?` | `ReviewSubmitResult` | ✅ **SQLite**（2.3 新增） |

> `review_submit` 在**一个事务**内完成：掌握度更新 → SM-2 重新排期 → 写 `review_logs`
> → 写 `mastery_events`（`source = "review"`）→ 覆盖 `entity_states`。
> 任一步失败整体回滚，不会出现"日志记了但状态没变"的中间态。
>
> 幂等：`submissionId` 非空且已存在时直接回放既有结果，不重复排期、不重复发 XP。
> `review_logs.xp` 是 XP 的唯一来源（`training_sessions.xp + review_logs.xp` 求和得等级），
> 因此"发 XP"就是写这一列，不需要额外的计数器表。

---

## 3. 数据结构约定

| 类型 | 说明 | 关键约束 |
|---|---|---|
| `Vocabulary` | 词条 | `id` 稳定；`mastery 0–100`；`dueAt` 用于复习队列 |
| `GrammarPoint` | 语法点 | 含 `structure` 公式与 `mistakes` 常见错误 |
| `Sentence` | 句子 | `chunks` 用于跟读分块 |
| `KnowledgeNode/Edge` | 图谱 | 节点 `id` 与实体 `id` 相同，坐标归一化 0–1 |
| `Exercise` | 题目 | 按 `kind` 区分的联合类型，每种题型字段不共用 |
| `ExerciseAnswer` | 作答 | `value` 可为 string / string[] / Record |
| `ExerciseResult` | 结果 | `accuracy`、`perKind`、`weakEntityIds` |
| `ReviewCard` | 复习卡片 | `preview` 给出四种自评各自的下次间隔；`answer/detail` 只在揭晓后展示 |
| `ReviewSubmitResult` | 复习结果 | `intervalDays=0` 表示 10 分钟后重学；`xp` 落 `review_logs.xp` |
| `StudyPlan` | 计划 | 每条建议带 `evidence[]` 以便解释 |
| `LearningRecord` / `DailyStat` | 学习事实表 | 所有统计都由它派生 |

所有实体 ID 前缀约定：`w_` 词、`g_` 语法、`s_` 句子、`u_` 用户、`lr_` 记录、
`ex_` 题目、`sess_` 训练会话、`pd_` 计划日、`pi_` 计划项、`rv_` 复习项。

---

## 4. Rust 侧实现示例

```rust
use serde_json::{json, Value};
use tauri::State;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
pub fn knowledge_detail(state: State<'_, AppState>, id: String) -> AppResult<Value> {
    let repo = state.repo();                      // 第二阶段的 SQLite 仓储
    repo.find_entity(&id)
        .ok_or_else(|| AppError::not_found(&id))  // → "not_found:找不到 xxx"
}
```

注册位置：`src-tauri/src/lib.rs` 的 `tauri::generate_handler![]`。
新增命令后必须同步：`contracts.ts` → `*.service.ts` → 本文件表格。

---

## 5. AI Core 接口（第二阶段）

```rust
// 建议的调用边界：AI 只返回结构化数据，绝不返回代码
#[derive(serde::Deserialize)]
pub struct AiPlanPayload {
    pub focus: String,
    pub items: Vec<PlanItemDto>,
    pub advice: Vec<AdviceDto>,
    pub model: String,
    pub generated_at: String,
}

#[tauri::command]
pub async fn plan_regenerate() -> AppResult<Value> {
    let raw = ai_core::complete(GENERATE_PLAN_PROMPT, user_profile()).await?;
    let payload: AiPlanPayload = serde_json::from_str(&raw)
        .map_err(|e| AppError::internal(format!("AI 返回结构不合法：{e}")))?;
    validate_plan(&payload)?;      // 业务校验：时长上限、知识点是否存在
    Ok(serde_json::to_value(payload)?)
}
```

前端拿到数据后：

1. 用 TS 类型做一次运行时校验（建议第二阶段引入 `zod`，只在服务层使用）。
2. 校验失败 → `ApiError { code: "internal" }`，页面显示 `ErrorState`，绝不把半成品
   数据渲染成 UI。
3. 永远不执行 AI 返回的字符串代码，也不动态生成组件。

---

## 6. 本地数据（第二阶段）

| 数据 | 建议存储 |
|---|---|
| 用户、设置 | `settings` 表（key-value）+ `profile` 表 |
| 词条 / 语法 / 句子 | 独立表 + `entity_relation` 关联表（图谱来源） |
| 学习记录 | `learning_record` 事实表，按日期索引 |
| 复习队列 | 由 `due_at` + `strength` 计算（SM-2 变体），不单独存 |
| 训练结果 | `exercise_result` + `exercise_answer` |

迁移与备份通过 `local_data_summary` / `local_data_clear` 暴露给设置页，路径由 Rust 管理，
前端不接触文件系统。
