-- 0003_ai：SAGE Intelligence 的 Provider 配置表。
--
-- 约定：
--   1. 只存"非密钥"配置。API Key 单独存放在应用数据目录的 provider-keys.json，
--      不进入业务库、不随数据库备份导出、不回传前端。
--   2. 本表与学习数据没有外键关系：AI 模块故障不影响任何学习功能。
--   3. 迁移只增不改（脚本校验和会被记录，改动即报错）。

CREATE TABLE ai_providers (
  id            TEXT PRIMARY KEY,
  display_name  TEXT NOT NULL,
  protocol      TEXT NOT NULL CHECK (protocol IN ('openai-compatible','anthropic')),
  base_url      TEXT NOT NULL,
  enabled       INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  default_model TEXT,
  models_json   TEXT NOT NULL DEFAULT '[]',
  has_api_key   INTEGER NOT NULL DEFAULT 0 CHECK (has_api_key IN (0,1)),
  updated_at    TEXT NOT NULL
);

CREATE INDEX idx_ai_providers_enabled ON ai_providers(enabled);
