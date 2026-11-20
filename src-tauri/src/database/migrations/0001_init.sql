-- 0001_init：第二阶段初始结构
-- 设计说明见 docs/DATABASE_DESIGN.md
-- 约定：
--   * 主键优先使用领域 id（TEXT），join 表用自增行号
--   * 所有时间存 ISO8601 UTC 定长文本（YYYY-MM-DDTHH:MM:SS.sssZ），字典序即时间序
--   * 内容表只增不删，下架用 deprecated_at
--   * 事件表只追加，不提供 UPDATE 路径

/* --------------------------------- 元数据 -------------------------------- */

CREATE TABLE app_metadata (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

/* ------------------------------- 账号与设置 ------------------------------- */

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
  key        TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

/* ----------------------------- 知识实体与内容 ----------------------------- */

CREATE TABLE entities (
  id            TEXT PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('word','phrase','grammar','sentence')),
  difficulty    INTEGER NOT NULL CHECK (difficulty BETWEEN 1 AND 5),
  level         TEXT NOT NULL,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  deprecated_at TEXT
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

/* --------------------------------- 题库 ---------------------------------- */

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
  item_count  INTEGER NOT NULL CHECK (item_count > 0),
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
  payload_json  TEXT NOT NULL,
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
  kind        TEXT NOT NULL,
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

/* -------------------------------- 学习态 --------------------------------- */

CREATE TABLE entity_states (
  entity_id        TEXT PRIMARY KEY REFERENCES entities(id) ON DELETE CASCADE,
  kind             TEXT NOT NULL CHECK (kind IN ('word','phrase','grammar','sentence')),
  mastery          REAL NOT NULL DEFAULT 0   CHECK (mastery BETWEEN 0 AND 100),
  strength         REAL NOT NULL DEFAULT 0   CHECK (strength BETWEEN 0 AND 1),
  state            TEXT NOT NULL DEFAULT 'new' CHECK (state IN
                     ('new','learning','reviewing','mastered','lapsed')),
  review_count     INTEGER NOT NULL DEFAULT 0 CHECK (review_count >= 0),
  lapse_count      INTEGER NOT NULL DEFAULT 0 CHECK (lapse_count >= 0),
  ease             REAL NOT NULL DEFAULT 2.5 CHECK (ease BETWEEN 1.3 AND 2.8),
  repetition       INTEGER NOT NULL DEFAULT 0 CHECK (repetition >= 0),
  interval_days    INTEGER NOT NULL DEFAULT 0 CHECK (interval_days BETWEEN 0 AND 180),
  due_at           TEXT,
  last_reviewed_at TEXT,
  row_version      INTEGER NOT NULL DEFAULT 1,
  updated_at       TEXT NOT NULL
);
CREATE INDEX idx_states_due ON entity_states(due_at) WHERE due_at IS NOT NULL;
CREATE INDEX idx_states_kind_state ON entity_states(kind, state);

/* -------------------------------- 学习事件 -------------------------------- */

-- 训练会话快照：training_start 时落库，training_submit 据此判分（前端无法篡改题目）
CREATE TABLE training_session_snapshots (
  session_id              TEXT PRIMARY KEY,
  kind                    TEXT NOT NULL,
  track                   TEXT NOT NULL CHECK (track IN ('vocabulary','grammar','speaking')),
  title                   TEXT NOT NULL,
  difficulty              INTEGER NOT NULL CHECK (difficulty BETWEEN 1 AND 5),
  estimated_minutes       INTEGER NOT NULL DEFAULT 0 CHECK (estimated_minutes >= 0),
  exercises_json          TEXT NOT NULL,
  target_entity_ids_json  TEXT NOT NULL DEFAULT '[]',
  started_at              TEXT NOT NULL,
  created_at              TEXT NOT NULL,
  consumed_at             TEXT
);

CREATE TABLE training_sessions (
  id            TEXT PRIMARY KEY,
  submission_id TEXT NOT NULL UNIQUE,
  session_id    TEXT NOT NULL,
  kind          TEXT NOT NULL,
  track         TEXT NOT NULL CHECK (track IN ('vocabulary','grammar','speaking')),
  title         TEXT NOT NULL,
  difficulty    INTEGER NOT NULL CHECK (difficulty BETWEEN 1 AND 5),
  minutes       INTEGER NOT NULL CHECK (minutes >= 0),
  duration_ms   INTEGER NOT NULL CHECK (duration_ms >= 0),
  score         REAL NOT NULL CHECK (score >= 0),
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
  exercise_id   TEXT NOT NULL,
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

CREATE TABLE training_session_entities (
  session_id TEXT NOT NULL REFERENCES training_sessions(id) ON DELETE CASCADE,
  entity_id  TEXT NOT NULL,
  PRIMARY KEY (session_id, entity_id)
);
CREATE INDEX idx_session_entities_entity ON training_session_entities(entity_id);

CREATE TABLE assessment_attempts (
  id              TEXT PRIMARY KEY,
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
  section_id TEXT NOT NULL,
  title      TEXT NOT NULL,
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
  elapsed_ms    INTEGER NOT NULL DEFAULT 0 CHECK (elapsed_ms >= 0),
  UNIQUE (attempt_id, exercise_id)
);

CREATE TABLE review_logs (
  id                 TEXT PRIMARY KEY,
  submission_id      TEXT,
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
  id             TEXT PRIMARY KEY,
  entity_id      TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  kind           TEXT NOT NULL CHECK (kind IN ('word','phrase','grammar','sentence')),
  mastery_before REAL NOT NULL CHECK (mastery_before BETWEEN 0 AND 100),
  mastery_after  REAL NOT NULL CHECK (mastery_after BETWEEN 0 AND 100),
  source         TEXT NOT NULL CHECK (source IN ('training','assessment','review')),
  ref_id         TEXT NOT NULL,
  at             TEXT NOT NULL
);
CREATE INDEX idx_mastery_entity_at ON mastery_events(entity_id, at DESC);
CREATE INDEX idx_mastery_at ON mastery_events(at DESC);

/* -------------------------------- 学习计划 -------------------------------- */

CREATE TABLE study_plans (
  id                               TEXT PRIMARY KEY,
  revision                         INTEGER NOT NULL CHECK (revision >= 1),
  status                           TEXT NOT NULL CHECK (status IN ('active','superseded')),
  generated_by                     TEXT NOT NULL CHECK (generated_by IN
                                     ('seed','regenerate','assessment','scheduled')),
  generated_at                     TEXT NOT NULL,
  model                            TEXT NOT NULL,
  difficulty_current               REAL NOT NULL CHECK (difficulty_current BETWEEN 1 AND 5),
  difficulty_suggested             REAL NOT NULL CHECK (difficulty_suggested BETWEEN 1 AND 5),
  difficulty_reason                TEXT NOT NULL,
  difficulty_requires_confirmation INTEGER NOT NULL DEFAULT 0
                                     CHECK (difficulty_requires_confirmation IN (0,1)),
  difficulty_override              REAL CHECK (difficulty_override BETWEEN 1 AND 5),
  superseded_at                    TEXT,
  created_at                       TEXT NOT NULL
);
CREATE UNIQUE INDEX ux_plan_active ON study_plans(status) WHERE status = 'active';

CREATE TABLE plan_days (
  id      TEXT PRIMARY KEY,
  plan_id TEXT NOT NULL REFERENCES study_plans(id) ON DELETE CASCADE,
  date    TEXT NOT NULL,
  focus   TEXT NOT NULL,
  UNIQUE (plan_id, date)
);
CREATE INDEX idx_plan_days_date ON plan_days(date);

CREATE TABLE plan_items (
  id               TEXT PRIMARY KEY,
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
  action_kind      TEXT,
  action_track     TEXT CHECK (action_track IS NULL OR action_track IN
                     ('vocabulary','grammar','speaking')),
  action_entity_id TEXT REFERENCES entities(id),
  UNIQUE (plan_day_id, sort_order),
  CHECK (action_type <> 'training' OR (action_kind IS NOT NULL AND action_track IS NOT NULL))
);
CREATE INDEX idx_plan_items_day_status ON plan_items(plan_day_id, status);

CREATE TABLE plan_weekly_goals (
  id         TEXT PRIMARY KEY,
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
  id            TEXT PRIMARY KEY,
  plan_id       TEXT NOT NULL REFERENCES study_plans(id) ON DELETE CASCADE,
  sort_order    INTEGER NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('vocabulary','grammar','speaking')),
  title         TEXT NOT NULL,
  body          TEXT NOT NULL,
  evidence_json TEXT NOT NULL DEFAULT '[]',
  action_label  TEXT NOT NULL,
  action_kind   TEXT NOT NULL,
  action_track  TEXT NOT NULL CHECK (action_track IN ('vocabulary','grammar','speaking')),
  UNIQUE (plan_id, sort_order)
);

/* -------------------------------- 读模型视图 ------------------------------- */

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
