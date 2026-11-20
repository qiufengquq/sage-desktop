# SQLite 数据库设计（第二阶段 · 阶段 A）

> 目标：把 `docs/DATA_MODEL_REVIEW.md` 里审查出的文档模型规范化为关系模型。
> 本文只给设计与 DDL，不含实现代码。DDL 是最终形态，实施时按迁移脚本拆分。

---

## 1. 设计原则

| 原则 | 落地方式 | 依据 |
|---|---|---|
| 三类数据严格分家 | **知识实体**（谁）/ **内容明细**（是什么）/ **学习态**（现在怎样）/ **事件**（发生过什么）/ **派生视图**（现算） | `DATA_MODEL_REVIEW.md` §3 |
| 事件只追加 | 事件表无 `updated_at`，只有 `created_at`；不提供 UPDATE 路径 | `learning-state.ts:9-13` 的既有约定 |
| 指标不落库 | 日统计、连击、趋势、掌握度分布、计划完成率全部现算 | `selectors.ts:141-268`、`learning-db.ts:370-379` |
| 幂等靠约束，不靠内存判断 | `UNIQUE(submission_id)` + 事务内 INSERT | `learning-db.ts:148` 的缺陷 D5 |
| 内容可演进、历史不可丢 | 内容行软删除（`deprecated_at`）；学习事件不删 | D14 |
| 时间统一 UTC 文本 | `YYYY-MM-DDTHH:MM:SS.sssZ`，定长，字典序即时间序 | 前端已是 `toISOString()` |
| 日历日单独存 | `plan_days.date` 用 `YYYY-MM-DD`（本地日），不参与时区换算 | 计划按"今天"取，必须是人看的日期 |
| 显式枚举 | 全部 `TEXT + CHECK`，不用魔法整数 | D3（前缀推断） |

### 1.1 为什么 `kind` 要冗余在 `entities` 与 `entity_states` 两处

`entities.kind` 是**事实**（这个 id 是什么类型）。`entity_states.kind` 是**读优化**：`knowledge_list` 要按 `kind` 过滤掌握度分布，如果没有这一列，每次都要 JOIN `entities`。冗余的代价是要保证两处一致 —— 由 `entity_states` 的写入路径统一从 `entities` 读取，不允许客户端传 `kind`。DDL 里用触发器把这条约束钉死（见 §3.6）。

---

## 2. 表清单与必要性

| # | 表 | 职责 | 必要性 |
|---|---|---|---|
| 1 | `schema_migrations` | 迁移版本记录 | 必须 |
| 2 | `app_metadata` | 安装 id、内容包版本、db_revision、种子时间 | 必须 |
| 3 | `users` | 账号身份（v2 只有一行） | 可选，见 §2.1 |
| 4 | `user_profiles` | 学习画像与目标 | 必须 |
| 5 | `app_settings` | 点分键值设置 | 必须 |
| 6 | `entities` | 知识实体身份 + 分类 | 必须 |
| 7 | `vocabulary` | 词/短语正文 | 必须 |
| 8 | `word_meanings` | 释义 | 必须（一对多） |
| 9 | `word_examples` | 例句 | 必须（一对多） |
| 10 | `grammar_points` | 语法点正文 | 必须 |
| 11 | `grammar_examples` | 语法例句 | 必须 |
| 12 | `grammar_mistakes` | 常见错误 | 必须 |
| 13 | `sentences` | 句子正文 | 必须 |
| 14 | `knowledge_relations` | 关系边（图谱/邻居/关联） | 必须（取代 3 个 id 数组） |
| 15 | `training_options` | 训练入口元数据 | 必须 |
| 16 | `exercises` | 题库（`payload_json` 承载题型专有字段） | 必须，见 §2.2 |
| 17 | `exercise_entities` | 题目 → 知识点（掌握度回写） | 必须 |
| 18 | `assessment_modules` | 测评模块 | 必须 |
| 19 | `assessment_sections` | 测评分项 | 必须 |
| 20 | `assessment_section_exercises` | 分项 → 题目 | 必须 |
| 21 | `entity_states` | **当前**掌握度与调度状态 | 必须 |
| 22 | `training_sessions` | 训练会话事实 | 必须 |
| 23 | `training_answers` | 逐题作答 | 必须（拆出，解决 D4） |
| 24 | `training_kind_scores` | 按题型得分 | 必须（`rebuildPlan` 依赖，`learning-db.ts:472-478`） |
| 25 | `assessment_attempts` | 测评事实 | 必须 |
| 26 | `assessment_section_results` | 分项成绩 | 必须 |
| 27 | `assessment_answers` | 测评逐题作答 | 必须 |
| 28 | `review_logs` | 每次复习的调度事实 | 必须 |
| 29 | `mastery_events` | 掌握度变化账本 | 必须（趋势回放，`stats.ts:masteryTrendOf`） |
| 30 | `study_plans` | 计划版本 | 必须 |
| 31 | `plan_days` | 计划日 | 必须 |
| 32 | `plan_items` | 计划项 | 必须 |
| 33 | `plan_weekly_goals` | 周目标 | 必须 |
| 34 | `plan_advice` | 建议（含证据） | 必须 |
| 35 | `training_session_snapshots` | 开始训练时的题目快照（Q2 方案 B） | 必须 |
| 36 | `knowledge_index`（VIEW） | 列表/图谱的统一读模型 | 必须 |

共 35 表 + 1 视图。

### 2.1 关于 `users` 与 `user_profiles` 拆分

现状是单机单用户（`mock/user.ts:8-19`）。拆两张表的唯一理由是"身份 vs 画像"职责不同，未来加多设备/多用户时不必改表。**代价**：v2 阶段每次读 profile 要 JOIN，且要维护一行 `users`。

如果你认为这是过度设计，可以合并为一张 `user_profiles(user_id PRIMARY KEY, display_name, handle, joined_at, ...)`。我倾向**保留拆分**，因为它让"画像可重新计算/可重置"与"账号不可变"的语义更清楚，且成本是常量级的。**这是一个待确认问题**（见实施计划 §7）。

### 2.2 关于 `exercises.payload_json`

11 种题型的字段差异很大（`types/exercise.ts:57-150`）：连线题要 `pairs[]`，选择题要 `options[]`，排序题要 `tokens[]`/`answer[]`，情景对话要 `turns[]`。三种做法：

| 方案 | 优点 | 缺点 | 结论 |
|---|---|---|---|
| A. 每种题型一张表 | 强类型、可 SQL 查询选项 | 11 张表 + 11 组 JOIN，43 行数据 | ✗ 过度设计 |
| B. 一张表 + `payload_json` | 与 TS 可辨识联合一一对应，序列化零损耗 | 无法用 SQL 过滤"选项文本" | ✓ **采纳** |
| C. 一张表 + 统一 options 子表 | 选项可查 | 只有 2 种题型有选项，其余仍要 JSON | ✗ 半吊子 |

选 B，并把**唯一的跨表查询需求**单独建模：`exercise_entities`（掌握度回写要知道这道题影响哪些实体，这是 SQL 能做的事）。

代价写清楚：Rust 判分时必须反序列化 `payload_json` 到 `enum ExercisePayload`，且该枚举要与 `types/exercise.ts` 保持同步 —— 由阶段 B 的"内容导出 + 契约测试"保证。

---

## 3. DDL

### 3.1 元数据

```sql
PRAGMA user_version = 1;          -- 与 schema_migrations 最新版本保持一致，作为快速检查

CREATE TABLE schema_migrations (
  version    INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  checksum   TEXT NOT NULL,        -- 迁移文件内容的 sha256，防止已应用脚本被篡改
  applied_at TEXT NOT NULL
);

CREATE TABLE app_metadata (
  key        TEXT PRIMARY KEY,     -- app_install_id | content_pack_version | content_pack_checksum
                                   -- | seeded_at | db_revision | last_backup_at
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

`db_revision` 每次写事务 +1，用于前端"数据变了要重新查询"的信号（对应现在的 zustand `revision`，`learning-store.ts:63`）。

### 3.2 账号与设置

```sql
CREATE TABLE users (
  id           TEXT PRIMARY KEY,
  handle       TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  joined_at    TEXT NOT NULL,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);

CREATE TABLE user_profiles (
  user_id       TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  level         TEXT NOT NULL CHECK (level IN ('A1','A2','B1','B2','C1','C2')),
  target_level  TEXT NOT NULL CHECK (target_level IN ('A1','A2','B1','B2','C1','C2')),
  goal          TEXT NOT NULL CHECK (goal IN ('exam','career','travel','academic','interest')),
  daily_minutes INTEGER NOT NULL CHECK (daily_minutes BETWEEN 5 AND 600),
  weekly_days   INTEGER NOT NULL CHECK (weekly_days BETWEEN 1 AND 7),
  timezone      TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

CREATE TABLE app_settings (
  key        TEXT PRIMARY KEY,     -- appearance.theme | appearance.accent | appearance.density
                                   -- | appearance.reduceMotion | reminder.enabled
                                   -- | reminder.time | reminder.weekdaysOnly | soundEffects
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

`app_settings` 用点分键而不是整块 JSON，是为了让 `settings_update` 的**深合并语义**（`user.service.ts:62-80`，逐层 merge）在 SQL 层就是 `UPSERT`，不需要"读出整块 → 改 → 写回"。

### 3.3 知识实体与内容

```sql
CREATE TABLE entities (
  id            TEXT PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('word','phrase','grammar','sentence')),
  difficulty    INTEGER NOT NULL CHECK (difficulty BETWEEN 1 AND 5),
  level         TEXT NOT NULL,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  deprecated_at TEXT                 -- 内容下架标记：不物理删除，保证历史外键有效
);
CREATE INDEX idx_entities_kind_difficulty ON entities(kind, difficulty);

CREATE TABLE vocabulary (
  entity_id     TEXT PRIMARY KEY REFERENCES entities(id) ON DELETE CASCADE,
  term          TEXT NOT NULL,
  phonetic      TEXT NOT NULL,
  pos           TEXT NOT NULL CHECK (pos IN
                  ('noun','verb','adjective','adverb','preposition','conjunction','pronoun','phrase')),
  contexts_json TEXT NOT NULL DEFAULT '[]'
);
CREATE UNIQUE INDEX ux_vocabulary_term ON vocabulary(term);

CREATE TABLE word_meanings (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  entity_id     TEXT NOT NULL REFERENCES vocabulary(entity_id) ON DELETE CASCADE,
  sort_order    INTEGER NOT NULL,
  pos           TEXT NOT NULL CHECK (pos IN
                  ('noun','verb','adjective','adverb','preposition','conjunction','pronoun','phrase')),
  definition    TEXT NOT NULL,
  definition_en TEXT,
  UNIQUE (entity_id, sort_order)
);

CREATE TABLE word_examples (
  id         TEXT PRIMARY KEY,
  entity_id  TEXT NOT NULL REFERENCES vocabulary(entity_id) ON DELETE CASCADE,
  sort_order INTEGER NOT NULL,
  en         TEXT NOT NULL,
  zh         TEXT NOT NULL,
  highlight  TEXT,
  UNIQUE (entity_id, sort_order)
);

CREATE TABLE grammar_points (
  entity_id   TEXT PRIMARY KEY REFERENCES entities(id) ON DELETE CASCADE,
  title       TEXT NOT NULL,
  title_en    TEXT NOT NULL,
  category    TEXT NOT NULL CHECK (category IN
                ('tense','clause','preposition','article','sentence-structure','modality','voice')),
  summary     TEXT NOT NULL,
  structure   TEXT NOT NULL,
  explanation TEXT NOT NULL
);
CREATE UNIQUE INDEX ux_grammar_title ON grammar_points(title);

CREATE TABLE grammar_examples (
  id         TEXT PRIMARY KEY,
  entity_id  TEXT NOT NULL REFERENCES grammar_points(entity_id) ON DELETE CASCADE,
  sort_order INTEGER NOT NULL,
  en         TEXT NOT NULL,
  zh         TEXT NOT NULL,
  note       TEXT,
  UNIQUE (entity_id, sort_order)
);

CREATE TABLE grammar_mistakes (
  id         TEXT PRIMARY KEY,
  entity_id  TEXT NOT NULL REFERENCES grammar_points(entity_id) ON DELETE CASCADE,
  sort_order INTEGER NOT NULL,
  wrong      TEXT NOT NULL,
  correct    TEXT NOT NULL,
  reason     TEXT NOT NULL,
  UNIQUE (entity_id, sort_order)
);

CREATE TABLE sentences (
  entity_id   TEXT PRIMARY KEY REFERENCES entities(id) ON DELETE CASCADE,
  en          TEXT NOT NULL,
  zh          TEXT NOT NULL,
  category    TEXT NOT NULL CHECK (category IN ('daily','academic','workplace','travel','opinion')),
  chunks_json TEXT NOT NULL DEFAULT '[]'
);
CREATE UNIQUE INDEX ux_sentences_en ON sentences(en);

CREATE TABLE knowledge_relations (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  source_id  TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  target_id  TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  relation   TEXT NOT NULL CHECK (relation IN
               ('related','synonym','antonym','derived','used-in','explains')),
  weight     REAL NOT NULL DEFAULT 1 CHECK (weight >= 0),
  CHECK (source_id <> target_id),
  UNIQUE (source_id, target_id, relation)
);
CREATE INDEX idx_relations_target ON knowledge_relations(target_id, relation);
```

关系映射（与 `mock/knowledge.ts:104-118` 完全一致）：

| 原字段 | 边 |
|---|---|
| `Vocabulary.relatedWordIds[]` | `(word → target, 'related', 1.0)` |
| `Vocabulary.grammarIds[]` | `(word → grammar, 'explains', 0.8)` |
| `GrammarPoint.relatedWordIds[]` | `(grammar → word, 'used-in', 0.7)` |
| `Sentence.grammarIds[]` | `(sentence → grammar, 'used-in', 1.1)` |
| `Sentence.wordIds[]` | `(sentence → word, 'used-in', 1.0)` |

图谱坐标（`KnowledgeNode.x/y`）**不落库**：它由 `seedPosition()` 确定性算法生成（`mock/knowledge.ts:25-35`），存下来只会产生"内容改了坐标没改"的漂移。Rust 侧用同一算法现算。

### 3.4 题库与测评内容

```sql
CREATE TABLE training_options (
  id          TEXT PRIMARY KEY,
  track       TEXT NOT NULL CHECK (track IN ('vocabulary','grammar','speaking')),
  kind        TEXT NOT NULL CHECK (kind IN ('word-match','multiple-choice','spelling','dictation',
                                            'cloze','translate','sentence-order','error-correction',
                                            'rewrite','shadowing','roleplay')),
  title       TEXT NOT NULL,
  description TEXT NOT NULL,
  icon        TEXT NOT NULL,
  minutes     INTEGER NOT NULL CHECK (minutes > 0),
  item_count  INTEGER NOT NULL CHECK (item_count > 0),   -- 声明值；实际取 min(item_count, 题库容量)
  difficulty  INTEGER NOT NULL CHECK (difficulty BETWEEN 1 AND 5),
  sort_order  INTEGER NOT NULL
);

CREATE TABLE exercises (
  id            TEXT PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('word-match','multiple-choice','spelling','dictation',
                                              'cloze','translate','sentence-order','error-correction',
                                              'rewrite','shadowing','roleplay')),
  difficulty    INTEGER NOT NULL CHECK (difficulty BETWEEN 1 AND 5),
  xp            INTEGER NOT NULL DEFAULT 0 CHECK (xp >= 0),
  instruction   TEXT NOT NULL,
  context       TEXT,
  explanation   TEXT NOT NULL DEFAULT '',
  tags_json     TEXT NOT NULL DEFAULT '[]',
  payload_json  TEXT NOT NULL,      -- 题型专有字段（options/pairs/tokens/answer/turns/chunks…）
  deprecated_at TEXT
);
CREATE INDEX idx_exercises_kind_difficulty ON exercises(kind, difficulty);

CREATE TABLE exercise_entities (
  exercise_id TEXT NOT NULL REFERENCES exercises(id) ON DELETE CASCADE,
  entity_id   TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  PRIMARY KEY (exercise_id, entity_id)
);
CREATE INDEX idx_exercise_entities_entity ON exercise_entities(entity_id);

CREATE TABLE assessment_modules (
  id          TEXT PRIMARY KEY CHECK (id IN ('vocabulary','grammar','speaking')),
  title       TEXT NOT NULL,
  subtitle    TEXT NOT NULL,
  description TEXT NOT NULL,
  minutes     INTEGER NOT NULL CHECK (minutes > 0),
  sort_order  INTEGER NOT NULL
);

CREATE TABLE assessment_sections (
  id          TEXT PRIMARY KEY,
  module_id   TEXT NOT NULL REFERENCES assessment_modules(id) ON DELETE CASCADE,
  title       TEXT NOT NULL,
  description TEXT NOT NULL,
  kind        TEXT NOT NULL,        -- 与 exercises.kind 同域
  skill       TEXT NOT NULL,
  sort_order  INTEGER NOT NULL,
  UNIQUE (module_id, sort_order)
);

CREATE TABLE assessment_section_exercises (
  section_id  TEXT NOT NULL REFERENCES assessment_sections(id) ON DELETE CASCADE,
  exercise_id TEXT NOT NULL REFERENCES exercises(id) ON DELETE CASCADE,
  sort_order  INTEGER NOT NULL,
  PRIMARY KEY (section_id, exercise_id)
);
```

### 3.5 学习态

```sql
CREATE TABLE entity_states (
  entity_id      TEXT PRIMARY KEY REFERENCES entities(id) ON DELETE CASCADE,
  kind           TEXT NOT NULL CHECK (kind IN ('word','phrase','grammar','sentence')),
  mastery        REAL NOT NULL DEFAULT 0   CHECK (mastery BETWEEN 0 AND 100),
  strength       REAL NOT NULL DEFAULT 0   CHECK (strength BETWEEN 0 AND 1),
  state          TEXT NOT NULL DEFAULT 'new' CHECK (state IN
                   ('new','learning','reviewing','mastered','lapsed')),
  review_count   INTEGER NOT NULL DEFAULT 0 CHECK (review_count >= 0),
  lapse_count    INTEGER NOT NULL DEFAULT 0 CHECK (lapse_count >= 0),
  ease           REAL NOT NULL DEFAULT 2.5 CHECK (ease BETWEEN 1.3 AND 2.8),
  repetition     INTEGER NOT NULL DEFAULT 0 CHECK (repetition >= 0),
  interval_days  INTEGER NOT NULL DEFAULT 0 CHECK (interval_days BETWEEN 0 AND 180),
  due_at         TEXT,
  last_reviewed_at TEXT,
  row_version    INTEGER NOT NULL DEFAULT 1,
  updated_at     TEXT NOT NULL
);
CREATE INDEX idx_states_due ON entity_states(due_at) WHERE due_at IS NOT NULL;
CREATE INDEX idx_states_kind_state ON entity_states(kind, state);
```

复习队列查询就是：

```sql
SELECT * FROM entity_states
WHERE due_at IS NOT NULL AND due_at <= ?1
ORDER BY due_at
LIMIT ?2;
```

`ease` 的上下界（1.3–2.8）直接来自 `SRS_PARAMS.minEase/maxEase`（`srs.ts:24-26`），用 `CHECK` 保证即使算法写错也不会把状态写坏。

### 3.6 学习事件

```sql
-- 训练会话快照：training_start 时落库，training_submit 据此判分（Q2 方案 B）
-- 前端提交时只传 sessionId + answers，题目与正确答案都不经过前端，判分不可篡改
CREATE TABLE training_session_snapshots (
  session_id             TEXT PRIMARY KEY,   -- sess_*
  kind                   TEXT NOT NULL,
  track                  TEXT NOT NULL CHECK (track IN ('vocabulary','grammar','speaking')),
  title                  TEXT NOT NULL,
  difficulty             INTEGER NOT NULL CHECK (difficulty BETWEEN 1 AND 5),
  estimated_minutes      INTEGER NOT NULL DEFAULT 0 CHECK (estimated_minutes >= 0),
  exercises_json         TEXT NOT NULL,      -- 取题快照（完整 Exercise 结构）
  target_entity_ids_json TEXT NOT NULL DEFAULT '[]',
  started_at             TEXT NOT NULL,
  created_at             TEXT NOT NULL,
  consumed_at            TEXT                -- 提交后写入，同一会话不可二次判分
);

CREATE TABLE training_sessions (
  id            TEXT PRIMARY KEY,          -- ta_*
  submission_id TEXT NOT NULL UNIQUE,      -- 幂等键（原 session.id）
  session_id    TEXT NOT NULL,             -- sess_*，便于追溯"这次训练属于哪个会话"
  kind          TEXT NOT NULL,
  track         TEXT NOT NULL CHECK (track IN ('vocabulary','grammar','speaking')),
  title         TEXT NOT NULL,
  difficulty    INTEGER NOT NULL CHECK (difficulty BETWEEN 1 AND 5),
  minutes       INTEGER NOT NULL CHECK (minutes >= 0),
  duration_ms   INTEGER NOT NULL CHECK (duration_ms >= 0),
  score         INTEGER NOT NULL CHECK (score >= 0),
  max_score     INTEGER NOT NULL CHECK (max_score >= 0),
  accuracy      INTEGER NOT NULL CHECK (accuracy BETWEEN 0 AND 100),
  xp            INTEGER NOT NULL DEFAULT 0 CHECK (xp >= 0),
  started_at    TEXT NOT NULL,
  finished_at   TEXT NOT NULL,
  created_at    TEXT NOT NULL
);
CREATE INDEX idx_sessions_finished ON training_sessions(finished_at DESC, id DESC);
CREATE INDEX idx_sessions_track ON training_sessions(track, finished_at DESC);

CREATE TABLE training_answers (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id    TEXT NOT NULL REFERENCES training_sessions(id) ON DELETE CASCADE,
  exercise_id   TEXT NOT NULL,      -- 故意不设 FK：种子历史含 ex_seed_* 合成 id（seed.ts:144-146）
  value_json    TEXT NOT NULL,
  correct       INTEGER NOT NULL CHECK (correct IN (0,1)),
  partial_score REAL NOT NULL CHECK (partial_score BETWEEN 0 AND 1),
  elapsed_ms    INTEGER NOT NULL DEFAULT 0 CHECK (elapsed_ms >= 0),
  UNIQUE (session_id, exercise_id)
);
CREATE INDEX idx_answers_exercise ON training_answers(exercise_id);

CREATE TABLE training_kind_scores (
  session_id TEXT NOT NULL REFERENCES training_sessions(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL,
  correct    INTEGER NOT NULL CHECK (correct >= 0),
  total      INTEGER NOT NULL CHECK (total >= 0),
  PRIMARY KEY (session_id, kind)
);

CREATE TABLE assessment_attempts (
  id              TEXT PRIMARY KEY,        -- aa_*
  submission_id   TEXT NOT NULL UNIQUE,
  module_id       TEXT NOT NULL REFERENCES assessment_modules(id),
  score           INTEGER NOT NULL CHECK (score BETWEEN 0 AND 100),
  accuracy        INTEGER NOT NULL CHECK (accuracy BETWEEN 0 AND 100),
  level           TEXT NOT NULL,
  strengths_json  TEXT NOT NULL DEFAULT '[]',
  weaknesses_json TEXT NOT NULL DEFAULT '[]',
  recommendation  TEXT NOT NULL DEFAULT '',
  minutes         INTEGER NOT NULL DEFAULT 0 CHECK (minutes >= 0),
  finished_at     TEXT NOT NULL,
  created_at      TEXT NOT NULL
);
CREATE INDEX idx_assessments_module ON assessment_attempts(module_id, finished_at DESC);

CREATE TABLE assessment_section_results (
  attempt_id TEXT NOT NULL REFERENCES assessment_attempts(id) ON DELETE CASCADE,
  section_id TEXT NOT NULL,                -- 不设 FK：允许分项被后续内容包删除后历史仍可读
  title      TEXT NOT NULL,                -- 冗余标题：分项改名后历史展示不变
  correct    INTEGER NOT NULL CHECK (correct >= 0),
  total      INTEGER NOT NULL CHECK (total >= 0),
  score      INTEGER NOT NULL CHECK (score BETWEEN 0 AND 100),
  PRIMARY KEY (attempt_id, section_id)
);

CREATE TABLE assessment_answers (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  attempt_id    TEXT NOT NULL REFERENCES assessment_attempts(id) ON DELETE CASCADE,
  exercise_id   TEXT NOT NULL,
  value_json    TEXT NOT NULL,
  correct       INTEGER NOT NULL CHECK (correct IN (0,1)),
  partial_score REAL NOT NULL CHECK (partial_score BETWEEN 0 AND 1),
  elapsed_ms    INTEGER NOT NULL DEFAULT 0,
  UNIQUE (attempt_id, exercise_id)
);

CREATE TABLE review_logs (
  id                 TEXT PRIMARY KEY,     -- rl_*
  submission_id      TEXT,                 -- 新增：解决 D6（双击"认识"不会记两次）
  entity_id          TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  rating             TEXT NOT NULL CHECK (rating IN ('again','hard','good','easy')),
  prev_due_at        TEXT,
  next_due_at        TEXT NOT NULL,
  prev_interval_days INTEGER NOT NULL CHECK (prev_interval_days >= 0),
  next_interval_days INTEGER NOT NULL CHECK (next_interval_days BETWEEN 0 AND 180),
  ease               REAL NOT NULL CHECK (ease BETWEEN 1.3 AND 2.8),
  mastery_before     REAL NOT NULL CHECK (mastery_before BETWEEN 0 AND 100),
  mastery_after      REAL NOT NULL CHECK (mastery_after BETWEEN 0 AND 100),
  xp                 INTEGER NOT NULL DEFAULT 0 CHECK (xp >= 0),
  at                 TEXT NOT NULL
);
CREATE UNIQUE INDEX ux_review_submission ON review_logs(submission_id) WHERE submission_id IS NOT NULL;
CREATE INDEX idx_review_entity_at ON review_logs(entity_id, at DESC);
CREATE INDEX idx_review_at ON review_logs(at DESC);

CREATE TABLE mastery_events (
  id             TEXT PRIMARY KEY,         -- me_*
  entity_id      TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  kind           TEXT NOT NULL CHECK (kind IN ('word','phrase','grammar','sentence')),
  mastery_before REAL NOT NULL CHECK (mastery_before BETWEEN 0 AND 100),
  mastery_after  REAL NOT NULL CHECK (mastery_after BETWEEN 0 AND 100),
  source         TEXT NOT NULL CHECK (source IN ('training','review')),
  ref_id         TEXT NOT NULL,            -- ta_* 或 rl_*
  at             TEXT NOT NULL
);
CREATE INDEX idx_mastery_entity_at ON mastery_events(entity_id, at DESC);
CREATE INDEX idx_mastery_at ON mastery_events(at DESC);
```

**`review_logs` 与 `mastery_events` 为什么都保留**（职责边界，非重复）：

| 表 | 回答的问题 | 粒度 |
|---|---|---|
| `review_logs` | "为什么下次复习排到 12 月 3 日？" | 一次复习一行，含调度前后值 |
| `mastery_events` | "这个知识点掌握度是怎么涨到 82 的？" | 一次实体更新一行，训练/复习都记 |

一次复习会产生 **1 条 `review_logs` + 1 条 `mastery_events`**；一次训练会产生 **1 条 `training_sessions` + N 条 `training_answers` + M 条 `mastery_events`**。二者不可互相替代：复习日志不含"哪道题"，掌握度账本不含"下次什么时候再来"。

### 3.7 学习计划

```sql
CREATE TABLE study_plans (
  id                             TEXT PRIMARY KEY,   -- plan_*
  revision                       INTEGER NOT NULL,   -- 同一用户第几版计划
  status                         TEXT NOT NULL CHECK (status IN ('active','superseded')),
  generated_by                   TEXT NOT NULL CHECK (generated_by IN
                                   ('seed','regenerate','assessment','scheduled')),
  generated_at                   TEXT NOT NULL,
  model                          TEXT NOT NULL,
  difficulty_current             REAL NOT NULL CHECK (difficulty_current BETWEEN 1 AND 5),
  difficulty_suggested           REAL NOT NULL CHECK (difficulty_suggested BETWEEN 1 AND 5),
  difficulty_reason              TEXT NOT NULL,
  difficulty_requires_confirmation INTEGER NOT NULL DEFAULT 0 CHECK (difficulty_requires_confirmation IN (0,1)),
  difficulty_override            REAL CHECK (difficulty_override BETWEEN 1 AND 5),
  superseded_at                  TEXT,
  created_at                     TEXT NOT NULL
);
-- 同一时刻只能有一份 active 计划（部分唯一索引，比应用层判重可靠）
CREATE UNIQUE INDEX ux_plan_active ON study_plans(status) WHERE status = 'active';

CREATE TABLE plan_days (
  id      TEXT PRIMARY KEY,                      -- pd_YYYY-MM-DD
  plan_id TEXT NOT NULL REFERENCES study_plans(id) ON DELETE CASCADE,
  date    TEXT NOT NULL,                         -- YYYY-MM-DD（本地日历日）
  focus   TEXT NOT NULL,
  UNIQUE (plan_id, date)
);
CREATE INDEX idx_plan_days_date ON plan_days(date);

CREATE TABLE plan_items (
  id               TEXT PRIMARY KEY,             -- pi_<date>_<n>
  plan_day_id      TEXT NOT NULL REFERENCES plan_days(id) ON DELETE CASCADE,
  sort_order       INTEGER NOT NULL,
  title            TEXT NOT NULL,
  activity         TEXT NOT NULL CHECK (activity IN
                     ('vocabulary','grammar','speaking','review','assessment','knowledge')),
  minutes          INTEGER NOT NULL CHECK (minutes BETWEEN 0 AND 240),
  xp               INTEGER NOT NULL DEFAULT 0 CHECK (xp >= 0),
  reason           TEXT NOT NULL DEFAULT '',
  done             INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0,1)),
  status           TEXT NOT NULL CHECK (status IN ('pending','in-progress','done','skipped')),
  action_type      TEXT NOT NULL CHECK (action_type IN ('review','training','knowledge')),
  action_kind      TEXT,                          -- 仅 action_type='training'
  action_track     TEXT CHECK (action_track IS NULL OR action_track IN
                     ('vocabulary','grammar','speaking')),
  action_entity_id TEXT REFERENCES entities(id),   -- 仅 review/knowledge
  UNIQUE (plan_day_id, sort_order),
  CHECK (action_type <> 'training' OR (action_kind IS NOT NULL AND action_track IS NOT NULL))
);
CREATE INDEX idx_plan_items_day_status ON plan_items(plan_day_id, status);

CREATE TABLE plan_weekly_goals (
  id         TEXT PRIMARY KEY,                    -- wg_*
  plan_id    TEXT NOT NULL REFERENCES study_plans(id) ON DELETE CASCADE,
  sort_order INTEGER NOT NULL,
  label      TEXT NOT NULL,
  target     REAL NOT NULL CHECK (target >= 0),
  current    REAL NOT NULL CHECK (current >= 0),
  unit       TEXT NOT NULL CHECK (unit IN ('分钟','天','词','题')),
  rationale  TEXT NOT NULL DEFAULT '',
  UNIQUE (plan_id, sort_order)
);

CREATE TABLE plan_advice (
  id           TEXT PRIMARY KEY,                  -- adv_*
  plan_id      TEXT NOT NULL REFERENCES study_plans(id) ON DELETE CASCADE,
  sort_order   INTEGER NOT NULL,
  kind         TEXT NOT NULL CHECK (kind IN ('vocabulary','grammar','speaking')),
  title        TEXT NOT NULL,
  body         TEXT NOT NULL,
  evidence_json TEXT NOT NULL DEFAULT '[]',
  action_label TEXT NOT NULL,
  action_kind  TEXT NOT NULL,                     -- 与 exercises.kind 同域
  action_track TEXT NOT NULL CHECK (action_track IN ('vocabulary','grammar','speaking')),
  UNIQUE (plan_id, sort_order)
);
```

`plan_items.action_*` 是对 `PlanItemAction` 可辨识联合（`types/plan.ts:50-53`）的展开。三条分支的必填性由 `CHECK` 表达：`training` 必须有 `kind + track`；其余两条 `kind/track` 为 `NULL`。

`completion`（`{done,total,rate}`）**不建列**，读取时：

```sql
SELECT COUNT(*) AS total, SUM(done) AS done FROM plan_items WHERE plan_day_id = ?1;
```

### 3.8 读模型视图

```sql
CREATE VIEW knowledge_index AS
SELECT
  e.id,
  e.kind,
  e.difficulty,
  e.level,
  COALESCE(v.term, g.title, s.en) AS primary_text,
  COALESCE(
    (SELECT m.definition FROM word_meanings m
     WHERE m.entity_id = v.entity_id ORDER BY m.sort_order LIMIT 1),
    g.title_en,
    s.zh
  ) AS secondary_text
FROM entities e
LEFT JOIN vocabulary     v ON v.entity_id = e.id
LEFT JOIN grammar_points g ON g.entity_id = e.id
LEFT JOIN sentences      s ON s.entity_id = e.id
WHERE e.deprecated_at IS NULL;
```

`knowledge_list` 的过滤条件（`kind / difficulty / mastery / keyword`）就是在 `knowledge_index JOIN entity_states` 上加 `WHERE`，`keyword` 用 `LIKE '%'||?||'%'` 同时匹配 `primary_text/secondary_text/kind`。

---

## 4. TS → DB 字段映射

### 4.1 `EntityState` → `entity_states`

| TS 字段 | 列 | 类型 | 备注 |
|---|---|---|---|
| `entityId` | `entity_id` | TEXT PK | |
| `kind` | `kind` | TEXT | 写入时从 `entities.kind` 取，不接受客户端传值 |
| `mastery` | `mastery` | REAL 0–100 | |
| `strength` | `strength` | REAL 0–1 | |
| `state` | `state` | TEXT | 5 值枚举 |
| `reviewCount` | `review_count` | INTEGER | |
| `lapseCount` | `lapse_count` | INTEGER | |
| `ease` | `ease` | REAL 1.3–2.8 | SM-2 |
| `repetition` | `repetition` | INTEGER | SM-2 |
| `intervalDays` | `interval_days` | INTEGER 0–180 | |
| `dueAt` | `due_at` | TEXT NULL | ISO UTC |
| `lastReviewedAt` | `last_reviewed_at` | TEXT NULL | |
| `updatedAt` | `updated_at` | TEXT | |
| — | `row_version` | INTEGER | 新增：乐观并发 |

### 4.2 `TrainingAttempt` → 3 张表

| TS 字段 | 目标 |
|---|---|
| `id, submissionId, sessionId, kind, track, title, difficulty, minutes, durationMs, score, maxScore, accuracy, xp, startedAt, finishedAt` | `training_sessions` 同名列 |
| `perKind[]` | `training_kind_scores` |
| `answers[]` | `training_answers` |
| `entityIds[]` | **不存**，由 `training_answers JOIN exercise_entities` 派生（避免第三次冗余） |

> `entityIds` 在种子历史里可能包含题库中不存在的实体（`seed.ts:168` 直接用 `record.entityIds`）。因此"派生"与"存快照"会有差异。决定：**存一份快照**，建 `training_session_entities(session_id, entity_id)`，理由见 §5.1。

### 4.3 其他

| TS | 目标 |
|---|---|
| `AssessmentAttempt.perSection[]` | `assessment_section_results` |
| `AssessmentAttempt.answers[]` | `assessment_answers` |
| `AssessmentAttempt.strengths/weaknesses[]` | `strengths_json / weaknesses_json`（只整体展示，不再按项查询） |
| `ReviewLog` | `review_logs` 同名列（新增 `submission_id`） |
| `MasteryEvent.before/after` | `mastery_before / mastery_after` |
| `StudyPlan.today` | `plan_days` 中 `date = 今天` 的那一行（**不单独存**） |
| `StudyPlan.week[]` | `plan_days` 全部行（含今天） |
| `StudyPlan.profile` | 读时计算（`ability` 算法），不落库 |
| `StudyPlan.completion` | 读时聚合 |
| `LearningDb.planMeta` | `study_plans.generated_by / difficulty_override / generated_at` |

---

## 5. 数据完整性与生命周期

### 5.1 为什么 `training_session_entities` 要存快照

前端 `TrainingAttempt.entityIds` 是"本次训练涉及的知识实体"（`learning-state.ts:61`），用于知识库跳转与"这次训练影响了什么"。若改为从 `exercise_entities` 派生，种子历史（用合成 `ex_seed_*`）会全部丢失，用户看到的历史记录点不进去。

→ 结论：新增

```sql
CREATE TABLE training_session_entities (
  session_id TEXT NOT NULL REFERENCES training_sessions(id) ON DELETE CASCADE,
  entity_id  TEXT NOT NULL,               -- 不设 FK：容忍历史中的已下架实体
  PRIMARY KEY (session_id, entity_id)
);
CREATE INDEX idx_session_entities_entity ON training_session_entities(entity_id);
```

这是"历史事实快照"与"当前内容"分离的典型：**事件表可以引用已不存在的实体，内容表不能反过来依赖事件**。

### 5.2 删除策略

| 数据 | 策略 | 理由 |
|---|---|---|
| 内容（vocabulary/grammar/sentences/exercises） | 软删除 `deprecated_at`，永不物理删除 | 历史事件引用它们 |
| `entity_states` | 永不删除；"清理数据"= 重置为初值（对应 `clearLearningRecords`，`learning-db.ts:514-535`） | 保持实体清单完整，用户看到"未学习"而非"消失" |
| 学习事件 | 追加；仅在 `local_data_clear` 时事务内 `DELETE` | 历史不可篡改 |
| `study_plans` | 重新生成时旧版置 `superseded`；保留最近 5 版 | 允许用户回看"上周的计划为什么这样" |
| 备份文件 | 保留最近 10 份 + 迁移前强制备份 | 见架构文档 §6 |

`local_data_clear` 的 SQL 顺序（单事务）：

```sql
DELETE FROM review_logs;
DELETE FROM mastery_events;
DELETE FROM training_answers;
DELETE FROM training_kind_scores;
DELETE FROM training_session_entities;
DELETE FROM training_sessions;
DELETE FROM assessment_answers;
DELETE FROM assessment_section_results;
DELETE FROM assessment_attempts;
DELETE FROM plan_items;
DELETE FROM plan_weekly_goals;
DELETE FROM plan_advice;
DELETE FROM plan_days;
DELETE FROM study_plans;
UPDATE entity_states SET mastery=0, strength=0, state='new', review_count=0, lapse_count=0,
       ease=2.5, repetition=0, interval_days=0, due_at=NULL, last_reviewed_at=NULL,
       row_version=row_version+1, updated_at=?1;
```

注意顺序：有外键的方向必须先删子表。

### 5.3 版本管理

| 版本 | 存放 | 作用 |
|---|---|---|
| 数据库结构版本 | `schema_migrations.version` + `PRAGMA user_version` | 决定执行哪些迁移 |
| 内容包版本 | `app_metadata.content_pack_version` | 内容更新但结构不变时，只同步内容 |
| 行级版本 | `entity_states.row_version` | 乐观并发：`UPDATE ... WHERE entity_id=? AND row_version=?` |
| 全局修订号 | `app_metadata.db_revision` | 通知前端刷新（对应 zustand `revision`） |

---

## 6. 索引设计依据

| 索引 | 支撑的查询 | 为什么需要 |
|---|---|---|
| `idx_states_due`（部分索引） | 复习队列 `WHERE due_at <= now ORDER BY due_at` | 最高频查询，部分索引跳过 `NULL`（新课未入队） |
| `idx_states_kind_state` | 首页掌握度分布、知识库按类型过滤 | 36 行时无所谓，但这是每次进首页都要跑的聚合 |
| `idx_sessions_finished` | 最近记录、分析页区间查询、分页 | 支持 `finished_at DESC, id DESC` 的 keyset 分页 |
| `idx_answers_exercise` | `training_start` 的"近期做过的题降权" | 替代 `training.service.ts:64-71` 的全表扫描 |
| `idx_review_entity_at` / `idx_mastery_entity_at` | 单实体历史、趋势回放 | 趋势图按实体取序列 |
| `idx_review_at` / `idx_mastery_at` | 分析页区间统计 | |
| `idx_plan_items_day_status` | 今日任务、完成率 | |
| `idx_relations_target` | 反向邻居查询 | 图谱点击"谁引用了我" |
| `ux_plan_active`（部分唯一） | 保证只有一份 active 计划 | 用约束代替应用层判重 |
| `ux_review_submission`（部分唯一） | 复习提交幂等 | D6 |

**不建**的索引：`vocabulary.term` 之外的模糊搜索不走索引（数据量小，`LIKE` 全扫 36 行 < 0.1ms），不引入 FTS5。

---

## 7. 分页与大数据量

只有两处会长期增长：`training_sessions`（每次训练 1 行）与 `review_logs`（每次复习 1 行）。按每天 3 次训练 + 40 次复习估算，一年约 1.5 万行 —— 仍在"全量查询也只要几毫秒"的量级。

因此：

- 现有 `analytics_query(range)` 保持不变，区间过滤 + 全量返回。
- **新增**一个分页命令承载"完整学习历史"（`Page<T>` 信封已经在 `types/api.ts:36-40` 定义但从未使用）：

```
learning_records_page(cursor?: string, limit: number = 20) -> Page<LearningRecord>
```

游标为 `base64(finished_at|id)`，SQL 用 keyset：

```sql
SELECT ... FROM training_sessions
WHERE (finished_at, id) < (?1, ?2)
ORDER BY finished_at DESC, id DESC
LIMIT ?3;
```

这是**新增契约**，需要你确认（见实施计划 §7 待确认问题）。

---

## 8. 与前端契约的对应关系（不变的部分）

数据库重建后，以下 DTO **一个字段都不改**，页面无需修改：

`UserProfile`、`AppSettings`、`KnowledgeSummary`、`KnowledgeDetail`、`TrainingOption`、`TrainingSession`、`ExerciseResult`、`AssessmentModule`、`AssessmentOutcome`、`ReviewQueuePayload`、`ReviewCard`、`ReviewSubmitResult`、`StudyPlan`、`DashboardData`、`AnalyticsData`、`LocalDataSummary`、`SyncStatus`。

需要新增/微调的契约（都向后兼容）：

| 契约 | 变更 | 原因 |
|---|---|---|
| `review_submit` | 入参新增可选 `submissionId` | D6 幂等 |
| `training_submit` | 入参改为 `{sessionId, kind, track, exercises, answers}` 或保留 `session` 整对象 | D13 对齐 |
| `learning_records_page` | 新增命令 | §7 分页 |
| `db_maintenance` | 新增命令：`{action: "backup"｜"restore"｜"vacuum"｜"integrity_check", path?}` | 备份/恢复/体检 |
| 各写命令返回值 | 附带 `dbRevision` | 前端 backend 模式下刷新 signal |

---

## 9. 一页速查（ER 关系）

```
users ─1:1─ user_profiles
entities ─1:1─ vocabulary ─1:N─ word_meanings / word_examples
         ─1:1─ grammar_points ─1:N─ grammar_examples / grammar_mistakes
         ─1:1─ sentences
entities ─N:M─ entities            (knowledge_relations)
entities ─1:1─ entity_states       (当前状态)
entities ─1:N─ mastery_events      (掌握度账本)

exercises ─N:M─ entities           (exercise_entities)
training_sessions ─1:N─ training_answers / training_kind_scores / training_session_entities
assessment_attempts ─1:N─ assessment_section_results / assessment_answers
review_logs (N:1 entities)

study_plans ─1:N─ plan_days ─1:N─ plan_items
            ─1:N─ plan_weekly_goals / plan_advice
```

---

## 10. 落地核对（2026-11-14）

本节记录"设计 vs 实际"的差异。实际 DDL 见 `src-tauri/src/database/migrations/0001_init.sql`（`version = 1`，`SCHEMA_VERSION = 1`）。

### 10.1 已落地

| 设计项 | 实际实现 | 状态 |
|---|---|---|
| 35 张业务表 + `schema_migrations` + 1 视图 | `0001_init.sql` 全部建齐；`database/mod.rs:187-201` 有表数量硬断言 | ✔ 一致 |
| WAL / `synchronous=NORMAL` / `foreign_keys=ON` / `busy_timeout=5s` / `cache_size` / `wal_autocheckpoint` | `database/pragmas.rs:19-33` | ✔ 一致 |
| 迁移版本表 + 校验和 + 逐迁移独立事务 | `schema_migrations(version,name,checksum,applied_at)`，SHA-256 比对，`TransactionBehavior::Immediate`（`migrate.rs:29-33,102-117`） | ✔ 一致 |
| 迁移前备份 | `backup::snapshot(..., "pre-migrate")`，仅在有数据时执行（`migrate.rs:90-93`） | ✔ 一致 |
| 拒绝降级启动 | `db_version > code_version` → `database_too_new`（`migrate.rs:65-68`） | ✔ 一致 |
| 内容包幂等同步 | 版本比对 + 15 表 UPSERT + 末尾写版本/校验和（`content.rs:153-189`） | ✔ 一致 |
| 索引清单（事件表时间索引、到期部分索引、唯一索引） | 全部存在，含 `ux_review_submission`、`ux_plan_active` 部分唯一索引 | ✔ 一致 |
| `knowledge_index` 视图 | 已建（`0001_init.sql:445-463`） | ✔ 一致 |

### 10.2 设计未覆盖 / 需在 v2 补齐

| # | 问题 | 现状 | 影响 |
|---|---|---|---|
| G1 | 5 处历史表缺外键：`training_answers.exercise_id`、`training_session_entities.entity_id`、`assessment_answers.exercise_id`、`assessment_section_results.section_id`、`training_sessions.session_id` | 建表时未加 | 内容变更后历史行悬空。**注意**：加 FK 会阻止"题库下线旧题"，因此建议改为 `ON DELETE RESTRICT` 或保留软删除（见 §11） |
| G2 | `entity_states.kind` 与 `entities.kind` 无一致性约束（原设计 §3.6 提到触发器，实际未建） | 无触发器 | 两处可漂移，由 `state_repository.rs:44` 无条件覆盖维持 |
| G3 | `assessment_sections` 无"分项角色"列 | 只能靠"有无题目"隐式判断 | 空分项语义脆弱（见 `LEARNING_DATA_MODEL.md` §6.3） |
| G4 | `option_item_count` 只按 `kind` 查询，任取一行（**2.2 已修复**：改按 `kind+track+sort_order`） | `exercise_repository.rs:138-145` | 注：`kind` 本身**不应**唯一（`multiple-choice` 跨 vocabulary/grammar 两个 track），约束加错了会让内容包同步失败 |
| G5 | `plan_items.done` 与 `status` 双标志 | 无一致性约束 | 完成率可能自相矛盾 |
| G6 | 派生列落库：`training_sessions.score/max_score/accuracy/xp/minutes`、`training_kind_scores`、`assessment_attempts.accuracy`（= `score`） | 写入时按当时算法计算 | 算法升级后历史与重算不一致，且无法区分"哪个对" |
| G7 | `review_logs` 表完整但零写入 | 仅被计数（`user_repository.rs:186`） | 复习链路空洞 |
| G8 | `study_plans` 仅被读 `difficulty`，`plan_*` 4 张子表零读写 | `training_service.rs:100-107` | 预留结构空转 |
| G9 | `entity_states.row_version` 写而不读 | `state_repository.rs:55` 每次 +1 | 无乐观锁效果 |
| G10 | 内容同步只按声明的冲突键 UPSERT，`ux_vocabulary_term` / `ux_grammar_title` / `ux_sentences_en` 未纳入 | `content.rs:31-46` | 内容包修正拼写/标题会约束冲突 |
| G11 | 内容同步列名由 JSON key 直拼 SQL，无白名单 | `content.rs:90-130` | 内容包多一个字段 → 运行期 SQL 错误 |

---

## 11. 迁移 v2 建议（`0002_model_alignment.sql`）

> 状态（2026-11-14）：**2.2 已落地**为 `src-tauri/src/database/migrations/0002_model_alignment.sql`，`SCHEMA_VERSION = 2`。
> 本文后续内容是"已实施 + 待实施"的合并视图，逐条标注状态。
> 原则：先清洗数据 → 再改结构 → 每步独立可回滚；v2 迁移前自动 `pre-migrate` 快照（机制已就绪）。
>
> | 项 | 状态 | 说明 |
> |---|---|---|
> | 分项角色 `section_role` | ✅ 已落地 | 内容包导出 + 老库回填 |
> | `plan_items` 去 `done` | ✅ 已落地 | 迁移内含 `done=1 → status='done'` 折并 |
> | 训练入口 kind 唯一 | ❌ 已推翻 | `kind` 跨 track 重复出现，唯一约束会让同步失败；改为查询层修复 + 复合索引 |
> | 4 张叶子表补外键 | ✅ 已落地 | 先清孤儿再重建；`training_sessions.session_id` 因级联限制改由服务层保证 |
> | `entity_states.row_version` | ⏳ 待定 | 2.4 决定"实现 CAS"或"删列" |

### 11.1 结构变更（按依赖顺序）

```sql
-- 1) 分项角色显式化（修 G3 / M11）
ALTER TABLE assessment_sections ADD COLUMN section_role TEXT NOT NULL DEFAULT 'graded'
  CHECK (section_role IN ('graded','summary'));

-- 2) 计划项单标志（修 G5 / M6）
UPDATE plan_items SET status = CASE WHEN done = 1 THEN 'done' ELSE status END;
ALTER TABLE plan_items DROP COLUMN done;

-- 3) 训练入口查询索引（修 G4）
--    注意：**不要**给 kind 建 UNIQUE —— multiple-choice 跨 track 重复出现，
--    唯一约束会让内容包同步直接失败。真正修复在仓储层（kind+track+sort_order）。
CREATE INDEX idx_training_options_kind_track ON training_options(kind, track, sort_order);

-- 4) 掌握度并发（修 G9 / M9）——或改为删除 row_version，二选一
--    保留则必须在 upsert 的 WHERE 中带 row_version

-- 5) 历史外键（修 G1）：先清洗孤儿行，再加约束
DELETE FROM training_answers
 WHERE exercise_id NOT IN (SELECT id FROM exercises);
-- 其余 4 张表同理；随后：
--   training_sessions(session_id) → training_session_snapshots(session_id) ON DELETE RESTRICT
--   training_answers(exercise_id) → exercises(id) ON DELETE RESTRICT
--   assessment_answers(exercise_id) → exercises(id) ON DELETE RESTRICT
--   assessment_section_results(section_id) → assessment_sections(id) ON DELETE RESTRICT
--   training_session_entities(entity_id) → entities(id) ON DELETE RESTRICT
```

> **为什么用 `RESTRICT` 而不是 `CASCADE`**：历史事件不可因题库下线而消失。内容的生命周期由 `deprecated_at` 软删除管理，物理删除只在"重置本地数据"时发生。

### 11.2 派生列治理（不删表，先降级）

| 表.列 | 处置 | 说明 |
|---|---|---|
| `training_sessions.accuracy` | 改为写入时由 `score / max_score` 计算，并在读模型里重新推导 | 保留列但只有一个公式 |
| `assessment_attempts.accuracy` | 删除（与 `score` 完全同值） | 或定义为"逐题正确率"，与 `score` 区分 |
| `training_kind_scores` | 降级为视图 `v_training_kind_scores` | `answers JOIN exercises GROUP BY kind` |
| `training_sessions.difficulty` | 写入时取快照 `difficulty`（修 N4） | 不改表结构，只改写入 |
| `entity_states.strength/state/interval_days` | 保留（热路径需要），但必须有唯一写入口 | 见 `RUST_DATABASE_ARCHITECTURE.md` §4 |

### 11.3 更新日志与回滚

- 版本号连续递增，脚本**只增不改**；已应用脚本被改动由校验和拦截（已实现）。
- 每个迁移在独立 IMMEDIATE 事务内执行；失败整体回滚并保留 `pre-migrate` 快照。
- 回滚策略：**不做自动降级迁移**（避免双向脚本腐化）。版本过高时 `database_too_new` 拒绝启动，由用户安装对应版本；开发期用备份文件还原。

---

## 12. 事务策略（落地版）

| 场景 | 隔离级别/行为 | 事务范围 | 实现位置 |
|---|---|---|---|
| 迁移 | `Immediate` | 单个迁移脚本 | `migrate.rs:102-117` |
| 内容包同步 | `Immediate` | 15 表 + 版本写入 | `content.rs:166-189` |
| 训练提交 | `Immediate` | 幂等检查 → 4 表 → 状态 → 事件 → 快照标记 | `training_service.rs:113-319` |
| 测评提交 | `Immediate` | 幂等检查 → 3 表 → 状态 → 事件 | `assessment_service.rs:25-226` |
| 复习提交（待实现） | `Immediate` | 幂等 → `review_logs` → 状态 → 事件 → 计划勾选 | 2.3 阶段 |
| 计划生成（待实现） | `Immediate` | 旧版 superseded → 新版 5 表 | 2.4 阶段 |
| 只读查询 | 无事务（`with_conn`） | 单条/批量 SELECT | `knowledge_service` 等 |

规则：
1. **写操作必须在 `with_tx` 内**，禁止在命令层裸 `execute`。
2. 事务内**不调用**可能长时间阻塞的算法（判分是纯 CPU，毫秒级，可接受）。
3. 单连接 `Mutex` 下事务天然串行，`busy_timeout=5s` 只对"另一进程占用"生效。
4. 事务提交前统一 `bump_revision`（`app_metadata.db_revision`），供前端缓存失效。
5. **当前缺陷**：只读补种（`ensure_rows_for_all_entities`）也走 `with_tx`，导致启动即 +1 revision（见 `BUSINESS_MODEL_REVIEW.md` R8）。修法：补种改用"仅当有缺失行时才开事务"，或引入独立 `maintenance_revision`。

---

## 13. 备份与恢复（落地版）

| 能力 | 实现 | 是否接线 |
|---|---|---|
| 在线备份（`Connection::backup`） | `database/backup.rs:52-62` | 迁移前自动调用 ✔ |
| 备份校验（open + `integrity_check`） | `backup.rs:106` | ✘ 无生产调用 |
| 恢复（替换文件 + 重新打开） | `backup.rs:130-149` | ✘ 无生产调用 |
| 保留策略（最近 10 份） | `backup.rs:65`（prune） | ✘ 无生产调用 |
| 健康检查（`quick_check`、页数、WAL 大小） | `pragmas.rs:36` | ✘ 无生产调用 |
| `last_backup_at` 元数据 | **无人写入** | ✘ `sync_status` 因此恒为 `offline` |

→ **结论：备份能力已具备，但只有"迁移前快照"这一条路径被使用。** 2.4/2.6 阶段需要接线：设置页"导出/恢复"、启动后静默备份、`last_backup_at` 写回。

