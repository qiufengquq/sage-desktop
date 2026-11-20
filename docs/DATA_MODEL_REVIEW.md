# 数据模型审查报告（第二阶段 · 阶段 A）

> 审查方式：逐文件阅读真实代码，不依据文档推断。所有结论都带 `文件:行` 证据。
> 审查时间：2026-11-14。代码基线：`src/` 与 `src-tauri/` 当前工作区（尚未纳入 git 跟踪）。

---

## 0. 结论摘要

| # | 结论 | 影响 |
|---|---|---|
| 1 | 项目**已经**做过一次"为 SQLite 预留"的建模：`LearningDb` 明确把内容态与学习态分开，事件只追加 | 第二阶段不是从零设计，而是把这份文档模型**规范化成关系表** |
| 2 | 但内容表（`Vocabulary`/`GrammarPoint`/`Sentence`）自身仍内嵌 `mastery/state/dueAt` 等学习字段 | 落库时必须拆成"内容表 + `entity_states`"，否则同一事实有两个写入点 |
| 3 | 事件对象把子集合内嵌成数组（`answers[]`、`perSection[]`、`weeklyGoals[]`、`plan.week[].items[]`） | 需拆子表；否则无法按题目/分项/日期查询，也无法做事务级幂等 |
| 4 | `LearningRecord`、`DashboardData`、`AnalyticsData`、`AbilityProfile` 都是**派生视图** | 不落库。落库就会与事件表双写不一致 |
| 5 | 幂等目前靠"读出全表再 `find`"（`learning-db.ts:148`、`:232`） | 落库后必须改成 `UNIQUE(submission_id)` + 事务内 `INSERT`，否则并发两次提交会重复计分 |
| 6 | 前端 `COMMANDS` 25 条与 Rust 注册 25 条**集合不一致**：前端有 `review_queue`/`review_submit`，Rust 未注册；Rust 有 `app_health`/`app_info`，前端未纳入契约常量 | backend 模式下复习功能直接 404 |
| 7 | 3 条命令的**入参结构前后端不一致**（`training_start`、`training_submit`、`assessment_submit`） | 这些命令不能"直接实现"，必须先对齐契约 |
| 8 | 实体 `kind` 依靠 id 前缀推断（`learning-db.ts:35-40`） | 落库必须写显式 `kind` 列，不要继续靠 `w_`/`g_` 字符串猜类型 |

数据规模（决定"要不要上连接池/分页"这类决策）：

| 内容 | 数量 | 证据 |
|---|---|---|
| 词汇（含短语） | 18（其中 `pos: "phrase"` 4 条） | `src/services/mock/vocabulary.ts` |
| 语法点 | 10 | `src/services/mock/grammar.ts` |
| 句子 | 8 | `src/services/mock/sentences.ts` |
| 知识实体合计 | **36** | 18 + 10 + 8，与图谱节点数一致 |
| 题目 | **43**（11 种题型） | `src/services/mock/exercises.ts` |
| 训练入口 | 12 | 同上 `trainingOptions` |
| 测评分项 | 3 模块 / 15 分项 | `src/services/mock/assessment.ts` |
| 种子学习记录 | 10 条 | `src/services/mock/learning.ts` |

> 这个量级是后面所有性能判断的基础：**单机、单用户、万级行以内**。任何"分布式/连接池/异步驱动"的复杂度都需要单独证明必要性。

---

## 1. 审查范围

| 范围 | 文件 |
|---|---|
| 类型定义 | `src/types/{api,user,vocabulary,grammar,sentence,knowledge,learning,learning-state,review,plan,exercise,assessment,dashboard,index}.ts` |
| 服务层 | `src/services/api/{contracts,transport,index}.ts` + 8 个 `*.service.ts` |
| 领域数据层 | `src/services/data/{repository,learning-db,selectors,seed}.ts` |
| 状态 | `src/stores/learning-store.ts` |
| 算法 | `src/lib/algorithms/{srs,mastery,ability,planner,stats,selection,xp}.ts`、`src/lib/grading.ts` |
| 内容（Mock 即内容源） | `src/services/mock/{vocabulary,grammar,sentences,exercises,assessment,learning,knowledge,planner,user,base}.ts` |
| Rust | `src-tauri/src/{lib,state,error}.rs`、`commands/{app,user,learning,knowledge,mod}.rs` |
| 配置 | `src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/capabilities/default.json` |
| 文档 | `docs/API_CONTRACTS.md`、`docs/FRONTEND_ARCHITECTURE.md` |
| 测试 | `tests/{algorithms,integration,lib,helpers}` 共 13 个文件 |

---

## 2. 当前分层与数据流（真实实现）

```
页面 (features/*)
   │  useQuery(service.getXxx)        src/hooks/use-query.ts
   ▼
services/api/*.service.ts             ← 唯一数据入口，返回 ApiResult<T>
   │  serve(command, mockFn, {args})  src/services/api/transport.ts:108
   ├── dataSource === "mock"  → 执行 mockFn（读 stores + data 层）
   └── dataSource === "backend" → invoke(command, args)  ← 需要 __TAURI_INTERNALS__
   ▼
services/data/*.ts                     ← 领域读（selectors）与写（learning-db）
   ▼
stores/learning-store.ts               ← 单一可变快照 db + revision
   ▼
services/data/repository.ts            ← 目前是 localStorage 单文档 JSON
```

关键事实：

1. **整库快照**：所有写入都是"取整份 `LearningDb` → 纯函数返回新对象 → 整体替换"（`learning-store.ts:58-64`）。注释已声明"对应未来 SQLite 的一个事务"（`learning-store.ts:15`）。
2. **持久化只有一个文档**：`localStorage["sage.learning.db"]`，含版本号校验（`repository.ts:34-48`），版本不匹配直接丢弃并重新种子化。
3. **Rust 侧目前只有内存 KV**：`state.rs:9-31` 的 `Mutex<HashMap<String, Value>>`，仅 `settings_get/settings_update` 使用（`commands/user.rs:26-57`），其余 23 条命令返回 `unavailable`。
4. **内容与题库完全在前端**：`services/mock/*`。Rust 目前没有任何内容数据，这也是"知识库命令无法在 backend 模式可用"的根因。

---

## 3. 实体清单

### 3.1 知识实体（内容态）

| 实体 | 主键 | 关键字段 | 嵌套集合 | 证据 |
|---|---|---|---|---|
| 词汇 `Vocabulary` | `id`（`w_` / `ph_`） | `term, phonetic, pos, difficulty(1-5), level, contexts[]` | `meanings[]`、`examples[]`、`relatedWordIds[]`、`grammarIds[]` | `types/vocabulary.ts:33-59` |
| 语法 `GrammarPoint` | `id`（`g_`） | `title, titleEn, category(7 种), difficulty, level, summary, structure, explanation` | `examples[]`、`mistakes[]`、`relatedWordIds[]` | `types/grammar.ts:36-56` |
| 句子 `Sentence` | `id`（`s_`） | `en, zh, category(5 种), difficulty, level` | `grammarIds[]`、`wordIds[]`、`chunks[]` | `types/sentence.ts:18-36` |

**问题 D1：内容对象自称"内容"，却携带学习态字段。**

```54:58:src/types/vocabulary.ts
  /** 下次复习时间（ISO） */
  dueAt: string | null;
  /** 记忆强度 0–1，用于遗忘曲线 */
  strength: number;
  addedAt: string;
  lastReviewedAt: string | null;
```

`mastery/state/reviewCount/dueAt/strength/lastReviewedAt` 在三个实体类型里各出现一次（`vocabulary.ts:48-58`、`grammar.ts:51-55`、`sentence.ts:31-35`）。运行时靠 `knowledge.service.ts:31-69` 的 `mergeWord/mergeGrammar/mergeSentence` 用 `entityStates` 覆盖。

→ 这是"读时合成"，本身正确；但**这些字段在内容表里不应存在列**，否则会出现两个可写来源。DB 设计中内容表不含这些列，合成在服务层完成。

**问题 D2：`relatedWordIds` / `grammarIds` / `wordIds` 三个字段语义上是同一种东西——关系边。**

`services/mock/knowledge.ts:104-118` 已经把三组 id 数组全部摊平成 `KnowledgeEdge`。关系类型有 6 种（`types/knowledge.ts:14-20`：`related | synonym | antonym | derived | used-in | explains`），但目前三元组（source, target, relation）里 `relation` 是硬编码在循环里的。

→ DB 应有一张 `knowledge_relations(source_id, target_id, relation, weight)`，取代三个 id 数组；图谱、邻居、详情关联全部从它查。

**问题 D3：`kind` 靠 id 前缀猜。**

```35:40:src/services/data/learning-db.ts
export function kindOfEntityId(entityId: string): EntityKind {
  if (entityId.startsWith("g_")) return "grammar";
  if (entityId.startsWith("s_")) return "sentence";
  if (entityId.startsWith("ph_")) return "phrase";
  return "word";
}
```

`ph_` 前缀实际并不存在——短语的 id 仍是 `w_` 开头，靠 `pos === "phrase"` 区分（`vocabulary.ts:74`、`knowledge.service.ts:77`）。所以这个函数对短语的判断分支是**死代码**，真实类型来自 seed 时的 `pos` 判断（`seed.ts:74`）。

→ DB 中 `kind` 必须是实列 + `CHECK` 约束，不依赖前缀。

### 3.2 学习态 `EntityState`

```18:39:src/types/learning-state.ts
export interface EntityState {
  entityId: string;
  kind: EntityKind;
  mastery: number;
  strength: number;
  state: MasteryState;
  reviewCount: number;
  lapseCount: number;
  ease: number;          // SM-2 难度因子 1.3–2.8
  repetition: number;    // SM-2 连续成功次数
  intervalDays: number;  // 当前间隔（天），0 = 重学阶段
  dueAt: string | null;
  lastReviewedAt: string | null;
  updatedAt: string;
}
```

这是**唯一**"当前状态"表，一个实体一行。语义清晰，可直接映射为一张表：`mastery/strength/ease` 用 `REAL`，`intervalDays/repetition/reviewCount/lapseCount` 用 `INTEGER`，`state` 用 `TEXT + CHECK`，`dueAt/lastReviewedAt/updatedAt` 用 ISO 文本。

注意 `ease`/`repetition`/`intervalDays` 三个字段来自 SM-2（`lib/algorithms/srs.ts:24-34`），是调度器的隐藏状态；`mastery`/`strength` 来自掌握度模型（`lib/algorithms/mastery.ts:20-43`）。两者**必须同行存储**，因为一次作答要同时更新它们。

### 3.3 学习事件

| 事件 | 主键 | 幂等键 | 内嵌子集合 | 证据 |
|---|---|---|---|---|
| `TrainingAttempt` | `id`（`ta_`） | `submissionId`（取 `session.id`，`learning-db.ts:147`） | `perKind[]`、`answers[]`、`entityIds[]` | `learning-state.ts:42-64` |
| `AssessmentAttempt` | `id`（`aa_`） | `submissionId` | `perSection[]`、`answers[]` | `:67-87` |
| `ReviewLog` | `id`（`rl_`） | **无** | — | `:107-121` |
| `MasteryEvent` | `id`（`me_`） | **无** | — | `:94-104` |

**问题 D4：`answers[]` 内嵌导致三类查询做不到。**

`TrainingAttempt.answers` 是完整数组（`learning-state.ts:59`）。当前代码要统计"某道题最近做过没有"，只能**全表遍历每一次答案**：

```64:71:src/services/api/training.service.ts
      const seenAt = new Map<string, number>();
      for (const attempt of db.trainingAttempts) {
        const time = new Date(attempt.finishedAt).getTime();
        for (const answer of attempt.answers) {
          const previous = seenAt.get(answer.exerciseId) ?? 0;
          if (time > previous) seenAt.set(answer.exerciseId, time);
        }
      }
```

数据量小的时候无所谓，但这是 O(总答题数) 的扫描，且无法索引。DB 化后应拆 `training_answers(session_id, exercise_id, ...)`，并加 `(exercise_id)` 索引。

**问题 D5：幂等靠内存判断，落库后必须换成约束。**

```147:152:src/services/data/learning-db.ts
  const submissionId = input.session.id;
  const existing = db.trainingAttempts.find((attempt) => attempt.submissionId === submissionId);

  if (existing) {
    return { db, duplicate: true, attempt: existing };
  }
```

单线程 + 内存快照下这个写法是安全的；一旦变成"两个 IPC 请求并发到达"，`find` 与 `INSERT` 之间就有窗口。DB 化后：`UNIQUE(submission_id)` + 事务内 `INSERT`，冲突即视为重复提交。

**问题 D6：`ReviewLog` / `MasteryEvent` 没有幂等键。**

`applyReviewSubmit`（`learning-db.ts:278-351`）每次调用都会新增一条日志，没有去重。前端 `review.submitReview` 只传 `{entityId, rating}`（`review.service.ts:56-59`）——**双击"认识"就会记两次复习**。这是必须修的数据完整性问题，不是性能问题。

**问题 D7：`perSection[]` 内嵌测评记录。**

`AssessmentAttempt.perSection` 是 `{sectionId, title, correct, total, score}[]`（`learning-state.ts:74-81`）。能力画像（`lib/algorithms/ability.ts`）与"最弱分项"推荐都依赖它。落库拆成 `assessment_section_results` 便于按分项做趋势。

### 3.4 派生视图（不应落库）

| 视图 | 生成位置 | 为什么不能落库 |
|---|---|---|
| `LearningRecord` | `selectors.ts:102-119`，由 `TrainingAttempt` 映射 | 与训练记录同源，落库即双写 |
| `DashboardData` / `DashboardSnapshot` | `selectors.ts:141-226`（现算：日统计、连击、掌握度分布、准确率） | 全部由事件 + 状态派生 |
| `AnalyticsData` | `selectors.ts:228-268`（daily/weekly/trends/forgettingCurve/timeBuckets） | 同上 |
| `AbilityProfile` | `lib/algorithms/ability.ts:buildAbilityProfile`，甚至**读取时重算**（`selectors.ts:271-283` 的 `decoratePlan`） | 落库会显示旧值 |
| `ReviewQueueItem` / `ReviewCard` | `selectors.ts:304-352`，由 `dueAt` + 内容即时生成 | 队列是"查询结果"，不是事实 |
| `LocalDataSummary.sizeKb` | `selectors.ts:285-294`，现在是 `JSON.stringify(db).length/1024` | DB 化后应改用 `PRAGMA page_count * page_size` 或文件大小 |

> 这一条是第二阶段最重要的边界：**事件与状态进库，指标与队列现算**。Rust 侧的 `dashboard_snapshot`、`analytics_query` 是"查询命令"，不是"读缓存表"。

### 3.5 学习计划

`StudyPlan`（`types/plan.ts:109-121`）结构：

```
StudyPlan
├─ profile: AbilityProfile        ← 派生（不落库）
├─ today: PlanDay                 ← 与 week[0] 重复（同一批 items，同一批 id）
├─ week: PlanDay[7]               ← plan_days
│   └─ items: PlanItem[]          ← plan_items
│        └─ action: 可辨识联合    ← action_type/action_kind/action_track/action_entity_id
├─ weeklyGoals: WeeklyGoal[]      ← plan_weekly_goals
├─ advice: PlanAdvice[]           ← plan_advice（含 evidence[]）
├─ difficulty: {current, suggested, reason, requiresConfirmation}
├─ completion: {done, total, rate}  ← 派生（items 计数）
├─ generatedAt / model
└─ PlanGenerationMeta（独立存在 LearningDb.planMeta）
```

证据：`mock/planner.ts:141-162`（`today` 与 `week[0]` 的 `date` 相同、items 相同）、`planner.ts:122`（item id 为 `pi_<date>_<index>`）。

**问题 D8：`today` 与 `week[0]` 是同一份数据的两次拷贝。**

落库时只需要一张 `plan_days`，`today` 由 `WHERE date = 今天` 得到。若照搬结构建表，会写成两份并在某次更新后不一致。

**问题 D9：`completion` 是派生值。**

`learning-db.ts:370-379` 与 `:390-401` 每次由 items 重算。DB 中不应存 `completion_done/total/rate`。

**问题 D10：计划项完成标记的匹配规则过宽。**

```358:365:src/services/data/learning-db.ts
  const matches = (item: PlanItem) => {
    if (item.done || item.action.type !== action.type) return false;
    if (item.action.type === "training" && action.type === "training") {
      return item.action.kind === action.kind;
    }
    return true;
  };
```

`review` 类型只要 `action.type` 相同就匹配，随后 `items.map(...)` 会把**所有**未完成的 review 项一次性标记完成（`:367-369`）。一次复习可能把当天 3 个复习任务都打勾。

→ DB 化时应明确"一个事件标记哪一条计划项"：建议改为 `UPDATE plan_items SET done=1 WHERE id = (SELECT id ... ORDER BY sort_order LIMIT 1)`，只标记一条，并在文档里写清规则。

### 3.6 用户与设置

| 类型 | 字段 | 证据 |
|---|---|---|
| `UserProfile` | `id, displayName, handle, joinedAt, level, targetLevel, goal, dailyMinutes, weeklyDays, timezone` | `types/user.ts:10-23` |
| `AppSettings` | `appearance{theme,accent,density,reduceMotion}`、`reminder{enabled,time,weekdaysOnly}`、`soundEffects` | `:28-48` |
| `SyncStatus` | `state, lastSyncedAt, pendingChanges, deviceName, storageUsedKb` | `:52-58` |

**问题 D11：Rust 侧把 `AppSettings` 的默认值又硬编码了一份。**

```9:24:src-tauri/src/commands/user.rs
fn default_settings() -> Value {
    json!({
        "appearance": { "theme": "system", ... },
```

前端有 `mock/user.ts:21-34` 的同款默认值。两份默认值迟早会漂移（例如新增 `density` 之外的字段）。DB 化后默认值应只在**迁移脚本**或 `app_settings` 的种子行里出现一次。

**问题 D12：`sync_status_get` 在 mock 模式下编造 `pendingChanges: 0` 与 `storageUsedKb`，Rust 侧却是 `unavailable`。**

`user.service.ts:86-102` 返回 `state: "idle"`（诚实地表示未同步），但 `sync_trigger` 直接抛 `not_implemented`（`:105-114`）。DB 化后 `storageUsedKb` 有真实来源（数据库文件大小），`pendingChanges` 在无云同步时应恒为 0 并保持 `state: "offline"` 或 `"idle"`，不要出现"看起来已同步"。

---

## 4. 前端 ⇄ Rust 契约核对

### 4.1 命令集合差异

| 侧 | 数量 | 明细 |
|---|---|---|
| 前端 `COMMANDS`（`contracts.ts:8-49`） | 25 | user 8 + dashboard 1 + assessment 2 + plan 4 + analytics 1 + knowledge 4 + training 3 + review 2 |
| Rust `generate_handler!`（`lib.rs:12-45`） | 25 | app 2 + user 8 + learning 11 + knowledge 4 |

两侧都是 25，但**不是同一批**：

- 前端有、Rust 无：`review_queue`、`review_submit` → backend 模式下复习页必然失败。
- Rust 有、前端无：`app_health`、`app_info` → 未进入契约常量表，容易漏测。

`contracts.ts:46` 的注释"Rust 侧在 2B 实现"说明这是已知缺口，但必须在阶段 B 之前补上，否则"25 个命令"这个基线本身是错的。

### 4.2 入参结构不一致（会直接导致 invoke 失败或静默丢参）

| 命令 | 前端 args | Rust 签名 | 问题 |
|---|---|---|---|
| `training_start` | `{kind, track, count}`（`training.service.ts:87`） | `(kind: String, track: String)`（`learning.rs:65`） | `count` 丢失；前端另有 `difficulty/title` 未传 |
| `training_submit` | `{sessionId, answers}`（`training.service.ts:113`） | `(session: Option<Value>, answers: Option<Value>)`（`learning.rs:71`） | **参数名与结构双重不符**：Rust 要整份 session，前端只给 id；而判分/回写掌握度需要 `exercises` |
| `assessment_submit` | `{moduleId, answers, submissionId?}`（`assessment.service.ts:168`） | `(module_id: Option<String>, answers: Option<Value>)`（`learning.rs:51`） | `submissionId` 丢失 → 幂等键丢失 |
| `plan_apply_difficulty` | `{level: number}`（`planner.service.ts:54`） | `level: f32`（`learning.rs:32`） | 一致（f32 精度足够），但应统一为 `f64` 避免边界四舍五入差异 |
| `analytics_query` | `{range}`（`analytics.service.ts:19`） | `range: Option<u16>` | 一致 |
| `knowledge_list` | `{filter}`（`knowledge.service.ts:197`） | `filter: Option<Value>`（`knowledge.rs:11`） | 一致但不强类型，无法在 Rust 侧校验 |
| `user_profile_update` / `settings_update` | `{patch}` | `patch: Value` | 一致 |

**命名约定**：Tauri 2 会自动把前端 camelCase 入参映射到 snake_case 形参，因此 `moduleId → module_id` 可用；但**返回结构不会自动转换**，必须靠 `#[serde(rename_all = "camelCase")]`（`commands/app.rs:3-10` 的 `AppInfo` 是唯一正确示例）。其余命令返回 `serde_json::Value`，键名靠手写，缺少编译期保障。

### 4.3 错误契约

`AppError` 序列化为 `"code:message"` 字符串（`error.rs:42-57`），前端 `transport.normalizeError` 按首个 `:` 切分（`transport.ts:36-46`）。这条链路是稳的，DB 错误可以在 Rust 侧映射为：

| SQLite 情况 | AppError code | 说明 |
|---|---|---|
| 迁移失败 / 表结构不符 | `internal` | `detail` 带 SQL 与版本号 |
| 数据库文件被占用超时 | `unavailable` | 可重试 |
| 唯一约束冲突（幂等命中） | 不算错误 | 返回既有记录，`ok: true` |
| 外键不存在（entity 不存在） | `not_found` | |
| 参数非法（kind 越界、rating 非法） | `invalid_argument` | |

---

## 5. 缺陷清单（按优先级）

| ID | 缺陷 | 证据 | 阻塞阶段 |
|---|---|---|---|
| D1 | 内容类型内嵌学习态字段 | `vocabulary.ts:48-58` 等 | 阶段 B（建表） |
| D2 | 关系被摊成 3 个 id 数组 | `knowledge.ts:104-118` | 阶段 B（建表） |
| D3 | `kind` 靠前缀推断，短语分支是死代码 | `learning-db.ts:35-40` | 阶段 B |
| D4 | 事件子集合内嵌，无法索引/分页 | `learning-state.ts:59,74-81` | 阶段 C |
| D5 | 幂等靠内存 `find` | `learning-db.ts:148,232` | 阶段 C（必须改约束） |
| D6 | 复习提交无幂等键，双击双记 | `review.service.ts:56-59`、`learning-db.ts:278` | 阶段 C（需改契约） |
| D7 | 测评分项内嵌 | `learning-state.ts:74-81` | 阶段 C |
| D8 | `plan.today` 与 `week[0]` 重复 | `mock/planner.ts:141-162` | 阶段 C |
| D9 | `completion` 派生值若落库会失真 | `learning-db.ts:370-379` | 阶段 C |
| D10 | 计划项完成匹配规则过宽，一次复习打勾多条 | `learning-db.ts:358-369` | 阶段 C |
| D11 | 默认设置在两处硬编码 | `user.rs:9-24`、`mock/user.ts:21-34` | 阶段 B |
| D12 | `review_queue`/`review_submit` 未注册；`app_*` 未进契约 | `contracts.ts:46-48`、`lib.rs:14-15` | 阶段 B（补契约） |
| D13 | 3 条命令入参结构不符 | 见 4.2 表 | 阶段 D（必须先改） |
| D14 | 无数据库/迁移/版本概念，版本不符直接丢数据重新种子化 | `repository.ts:38`、`store:50-56` | 阶段 B |

### 关于 D14 的补充

```38:38:src/services/data/repository.ts
  if (candidate.schemaVersion !== SCHEMA_VERSION) return null;
```

`return null` 之后 `learning-store.init` 会 `buildSeedDb` 并覆盖写入（`learning-store.ts:52-55`）。也就是说：**任何一次结构升级都会静默清空用户的学习历史**。这在 localStorage 阶段是可接受的取舍，但在 SQLite 阶段绝对不行——迁移必须"保留数据 + 可回滚 + 失败时不覆盖"。

---

## 6. 现状中值得保留的部分

不是所有东西都要改。以下几点在第二阶段应原样保留：

1. **分层不变**：`features → services/api → (mock | invoke)`。SQLite 只在 Rust 侧，页面与服务契约不动（`FRONTEND_ARCHITECTURE.md:56-62`）。
2. **`ApiResult` 信封不变**：命令仍返回 `Result<T, AppError>`，前端仍拿到 `{ok, data} | {ok:false, error}`。
3. **纯函数算法层可复用为"对照实现"**：`lib/algorithms/*` 与 `lib/grading.ts` 都用显式 `now: Date` 入参（`srs.ts:68-72`、`mastery.ts:131`），天然可做 golden test 的双端一致性验证。
4. **`LearningRepository` 接口三方法**（`repository.ts:10-14`）：`load/save/clear` 的语义可以收敛成 Rust 仓储的 `bootstrap/reset`，前端 mock 侧继续用内存实现跑测试（`createMemoryRepository`，`repository.ts:83-94`）。
5. **`SCHEMA_VERSION` 概念**继续存在，只是从"不匹配就丢弃"改成"按 `schema_migrations` 逐步升级"。
6. **事件只追加、指标现算**的原则（`learning-state.ts:9-13` 的设计说明）本身是对的，DB 设计沿用。

---

## 7. 移交下一份文档

本报告只做"现状是什么、哪里有坑"。据此产出：

- `docs/DATABASE_DESIGN.md`：表结构、约束、索引、删除与历史保留、TS ↔ DB 字段映射。
- `docs/RUST_DATABASE_ARCHITECTURE.md`：rusqlite vs SQLx、连接管理、线程模型、迁移、事务、错误、备份。
- `docs/PHASE_2_IMPLEMENTATION_PLAN.md`：阶段 B–E 的任务拆分、影响文件、验收与风险。

**阶段 A 到此暂停，等待确认后再动代码。**
