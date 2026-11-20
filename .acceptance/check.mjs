import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const dbPath = join(dirname(fileURLToPath(import.meta.url)), "sage-acceptance.db");
const db = new DatabaseSync(dbPath, { readOnly: true });
const one = (sql) => db.prepare(sql).get();

const report = {
  userVersion: one("PRAGMA user_version").user_version,
  tables: one("SELECT COUNT(*) AS c FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'").c,
  views: one("SELECT COUNT(*) AS c FROM sqlite_master WHERE type='view'").c,
  migrations: one("SELECT COUNT(*) AS c FROM schema_migrations").c,
  entities: one("SELECT COUNT(*) AS c FROM entities").c,
  entityStates: one("SELECT COUNT(*) AS c FROM entity_states").c,
  users: one("SELECT COUNT(*) AS c FROM users").c,
  profiles: one("SELECT COUNT(*) AS c FROM user_profiles").c,
  settings: one("SELECT COUNT(*) AS c FROM app_settings").c,
  dueReady: one("SELECT COUNT(*) AS c FROM entity_states WHERE due_at IS NOT NULL").c,
  reviewLogs: one("SELECT COUNT(*) AS c FROM review_logs").c,
  masteryEvents: one("SELECT COUNT(*) AS c FROM mastery_events").c,
  revision: one("SELECT value FROM app_metadata WHERE key='db_revision'")?.value ?? null,
};

console.log(JSON.stringify(report, null, 2));
db.close();
