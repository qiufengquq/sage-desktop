import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const dbPath = join(dirname(fileURLToPath(import.meta.url)), "sage-acceptance.db");
const db = new DatabaseSync(dbPath, { readOnly: true });
const all = (sql, ...args) => db.prepare(sql).all(...args);
const one = (sql, ...args) => db.prepare(sql).get(...args);

const report = {
  reviewLogs: one("SELECT COUNT(*) AS c FROM review_logs").c,
  reviewXp: one("SELECT COALESCE(SUM(xp),0) AS c FROM review_logs").c,
  masteryEvents: one("SELECT COUNT(*) AS c FROM mastery_events WHERE source='review'").c,
  dueNow: one("SELECT COUNT(*) AS c FROM entity_states WHERE due_at IS NOT NULL AND due_at <= ?", new Date().toISOString()).c,
  logs: all(
    `SELECT entity_id, rating, prev_interval_days, next_interval_days, ease,
            mastery_before, mastery_after, xp, submission_id, at
       FROM review_logs ORDER BY at`,
  ),
  states: all(
    `SELECT entity_id, mastery, state, review_count, lapse_count, ease, repetition,
            interval_days, due_at, last_reviewed_at
       FROM entity_states WHERE review_count > 2 ORDER BY entity_id`,
  ),
};

console.log(JSON.stringify(report, null, 2));
db.close();
