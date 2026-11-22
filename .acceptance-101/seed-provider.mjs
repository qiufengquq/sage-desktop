import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const dbPath = join(dirname(fileURLToPath(import.meta.url)), "sage.db");
const db = new DatabaseSync(dbPath);

// 模拟"用户在设置页保存了一个 Provider，但还没填密钥"
db.prepare(
  `INSERT INTO ai_providers (id, display_name, protocol, base_url, enabled, default_model, models_json, has_api_key, updated_at)
   VALUES (?, ?, ?, ?, 1, ?, ?, 0, ?)
   ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name, updated_at = excluded.updated_at`,
).run(
  "offline",
  "本地占位 Provider",
  "openai-compatible",
  "http://127.0.0.1:1",
  "test-model",
  JSON.stringify(["test-model"]),
  new Date().toISOString(),
);

console.log(JSON.stringify(db.prepare("SELECT id, has_api_key FROM ai_providers").all()));
db.close();
