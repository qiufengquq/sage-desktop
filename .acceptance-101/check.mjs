import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const dbPath = join(dirname(fileURLToPath(import.meta.url)), "sage.db");
const db = new DatabaseSync(dbPath, { readOnly: true });
const one = (sql, ...args) => db.prepare(sql).get(...args);

const report = {
  userVersion: one("PRAGMA user_version").user_version,
  tables: one("SELECT COUNT(*) AS c FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'").c,
  migrations: db.prepare("SELECT version, name FROM schema_migrations ORDER BY version").all(),
  entities: one("SELECT COUNT(*) AS c FROM entities").c,
  entityStates: one("SELECT COUNT(*) AS c FROM entity_states").c,
  users: one("SELECT COUNT(*) AS c FROM users").c,
  settings: one("SELECT COUNT(*) AS c FROM app_settings").c,
  aiProviders: one("SELECT COUNT(*) AS c FROM ai_providers").c,
  aiProviderColumns: db.prepare("PRAGMA table_info(ai_providers)").all().map((r) => r.name),
};

console.log(JSON.stringify(report, null, 2));
db.close();
