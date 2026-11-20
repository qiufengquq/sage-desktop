import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const dbPath = join(dirname(fileURLToPath(import.meta.url)), "sage-acceptance.db");
const db = new DatabaseSync(dbPath);

const DAY = 24 * 60 * 60 * 1000;
const now = Date.now();
const iso = (ms) => new Date(ms).toISOString();
const dueAt = iso(now - 2 * DAY);
const lastAt = iso(now - 3 * DAY);

const targets = db
  .prepare("SELECT id, kind FROM entities WHERE kind IN ('word','grammar') ORDER BY kind DESC, id LIMIT 2")
  .all();

const update = db.prepare(
  `UPDATE entity_states SET
     mastery = 48, strength = 0.45, state = 'reviewing',
     review_count = 2, ease = 2.5, repetition = 2, interval_days = 5,
     due_at = ?, last_reviewed_at = ?, updated_at = ?,
     row_version = row_version + 1
   WHERE entity_id = ?`,
);

for (const target of targets) {
  update.run(dueAt, lastAt, iso(now), target.id);
}

console.log(
  JSON.stringify(
    {
      seeded: targets,
      dueCount: db.prepare("SELECT COUNT(*) AS c FROM entity_states WHERE due_at IS NOT NULL AND due_at <= ?").get(iso(now)).c,
      reviewLogs: db.prepare("SELECT COUNT(*) AS c FROM review_logs").get().c,
    },
    null,
    2,
  ),
);
db.close();
