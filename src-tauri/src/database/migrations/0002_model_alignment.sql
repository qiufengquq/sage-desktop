-- 0002_model_alignment：把 2.1 审查出的"模型级不一致"收敛掉。
--
-- 约定：
--   1. 只做结构性对齐，不改任何业务口径（分数/掌握度算法不动）。
--   2. 所有破坏性操作前先清洗孤儿行，避免 FK 建立失败。
--   3. SQLite 的限制：事务内 PRAGMA foreign_keys 无效，且 DROP TABLE 父表会
--      触发子表 ON DELETE CASCADE。因此本迁移只重建"叶子表"，父表（training_sessions）
--      的外键由服务层保证（training_session_snapshots.consumed_at 已是硬校验）。
--      详见 docs/DATABASE_DESIGN.md §11.1。

/* --------------------------------------------------------------------------
   1) 分项角色显式化
   --------------------------------------------------------------------------
   背景：sec_speak_eval 是"汇总/评估"分项，按设计不带题目。
   之前只能靠"有没有题"隐式判断，既脆弱又容易被误当成脏数据。

   graded = 可作答，参与总分
   summary = 汇总项，不判分、不参与总分、不占作答步骤
   -------------------------------------------------------------------------- */

ALTER TABLE assessment_sections
  ADD COLUMN section_role TEXT NOT NULL DEFAULT 'graded'
  CHECK (section_role IN ('graded', 'summary'));

-- 老库回填：没有任何题目的分项即汇总项（与内容包的语义一致）
UPDATE assessment_sections
   SET section_role = 'summary'
 WHERE id NOT IN (SELECT DISTINCT section_id FROM assessment_section_exercises);

/* --------------------------------------------------------------------------
   2) 计划项去掉双标志
   done 与 status 表达同一事实，必然漂移。统一保留 status。
   （plan_items 当前无任何读写路径，改动风险为 0）
   -------------------------------------------------------------------------- */

UPDATE plan_items SET status = 'done' WHERE done = 1 AND status <> 'done';
ALTER TABLE plan_items DROP COLUMN done;

/* --------------------------------------------------------------------------
   3) 训练入口：kind 不建唯一索引
   --------------------------------------------------------------------------
   审查时的判断（"kind 应唯一"）经数据核对被证伪：`multiple-choice` 同时存在于
   vocabulary 与 grammar 两个 track（12 个入口 / 11 种 kind）。
   真正的缺陷是"按 kind 单条件查询会任取一行"，已在仓储层修复：
   `exercise_repository::option_item_count(kind, track)` + ORDER BY sort_order LIMIT 1。
   因此本迁移不加约束，只补一个复合索引支撑该查询。
   -------------------------------------------------------------------------- */

CREATE INDEX idx_training_options_kind_track ON training_options(kind, track, sort_order);

/* --------------------------------------------------------------------------
   4) 历史表外键补齐（叶子表重建）
   目标：内容（题库/分项）变更不得让历史行悬空。
   策略：ON DELETE RESTRICT —— 历史不可因题库下线而消失；
        内容的生命周期由 deprecated_at 软删除管理。
   -------------------------------------------------------------------------- */

-- 4.0 先清洗孤儿行（内容是只增不删的，正常情况下应为 0 行）
DELETE FROM training_answers
 WHERE exercise_id NOT IN (SELECT id FROM exercises)
    OR session_id  NOT IN (SELECT id FROM training_sessions);

DELETE FROM training_session_entities
 WHERE entity_id  NOT IN (SELECT id FROM entities)
    OR session_id NOT IN (SELECT id FROM training_sessions);

DELETE FROM assessment_answers
 WHERE exercise_id NOT IN (SELECT id FROM exercises)
    OR attempt_id  NOT IN (SELECT id FROM assessment_attempts);

DELETE FROM assessment_section_results
 WHERE section_id NOT IN (SELECT id FROM assessment_sections)
    OR attempt_id NOT IN (SELECT id FROM assessment_attempts);

-- 4.1 training_answers.exercise_id → exercises(id)
CREATE TABLE training_answers_mig (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id    TEXT NOT NULL REFERENCES training_sessions(id) ON DELETE CASCADE,
  exercise_id   TEXT NOT NULL REFERENCES exercises(id) ON DELETE RESTRICT,
  value_json    TEXT NOT NULL,
  correct       INTEGER NOT NULL CHECK (correct IN (0,1)),
  partial_score REAL NOT NULL CHECK (partial_score BETWEEN 0 AND 1),
  elapsed_ms    INTEGER NOT NULL DEFAULT 0 CHECK (elapsed_ms >= 0),
  UNIQUE (session_id, exercise_id)
);
INSERT INTO training_answers_mig (id, session_id, exercise_id, value_json, correct, partial_score, elapsed_ms)
SELECT id, session_id, exercise_id, value_json, correct, partial_score, elapsed_ms
  FROM training_answers;
DROP TABLE training_answers;
ALTER TABLE training_answers_mig RENAME TO training_answers;
CREATE INDEX idx_answers_exercise ON training_answers(exercise_id);

-- 4.2 training_session_entities.entity_id → entities(id)
CREATE TABLE training_session_entities_mig (
  session_id TEXT NOT NULL REFERENCES training_sessions(id) ON DELETE CASCADE,
  entity_id  TEXT NOT NULL REFERENCES entities(id) ON DELETE RESTRICT,
  PRIMARY KEY (session_id, entity_id)
);
INSERT INTO training_session_entities_mig (session_id, entity_id)
SELECT session_id, entity_id FROM training_session_entities;
DROP TABLE training_session_entities;
ALTER TABLE training_session_entities_mig RENAME TO training_session_entities;
CREATE INDEX idx_session_entities_entity ON training_session_entities(entity_id);

-- 4.3 assessment_answers.exercise_id → exercises(id)
CREATE TABLE assessment_answers_mig (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  attempt_id    TEXT NOT NULL REFERENCES assessment_attempts(id) ON DELETE CASCADE,
  exercise_id   TEXT NOT NULL REFERENCES exercises(id) ON DELETE RESTRICT,
  value_json    TEXT NOT NULL,
  correct       INTEGER NOT NULL CHECK (correct IN (0,1)),
  partial_score REAL NOT NULL CHECK (partial_score BETWEEN 0 AND 1),
  elapsed_ms    INTEGER NOT NULL DEFAULT 0 CHECK (elapsed_ms >= 0),
  UNIQUE (attempt_id, exercise_id)
);
INSERT INTO assessment_answers_mig (id, attempt_id, exercise_id, value_json, correct, partial_score, elapsed_ms)
SELECT id, attempt_id, exercise_id, value_json, correct, partial_score, elapsed_ms
  FROM assessment_answers;
DROP TABLE assessment_answers;
ALTER TABLE assessment_answers_mig RENAME TO assessment_answers;

-- 4.4 assessment_section_results.section_id → assessment_sections(id)
CREATE TABLE assessment_section_results_mig (
  attempt_id TEXT NOT NULL REFERENCES assessment_attempts(id) ON DELETE CASCADE,
  section_id TEXT NOT NULL REFERENCES assessment_sections(id) ON DELETE RESTRICT,
  title      TEXT NOT NULL,
  correct    INTEGER NOT NULL CHECK (correct >= 0),
  total      INTEGER NOT NULL CHECK (total >= 0),
  score      INTEGER NOT NULL CHECK (score BETWEEN 0 AND 100),
  PRIMARY KEY (attempt_id, section_id)
);
INSERT INTO assessment_section_results_mig (attempt_id, section_id, title, correct, total, score)
SELECT attempt_id, section_id, title, correct, total, score
  FROM assessment_section_results;
DROP TABLE assessment_section_results;
ALTER TABLE assessment_section_results_mig RENAME TO assessment_section_results;
