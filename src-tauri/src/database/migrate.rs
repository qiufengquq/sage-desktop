//! 版本化迁移。
//!
//! 规则：
//! 1. 迁移文件只增不改，编号连续；脚本内容参与校验和，已应用的脚本被改动即报错。
//! 2. 每条迁移在独立事务内执行（SQLite 的 DDL 是事务性的），失败不留半套表。
//! 3. 已有数据时，迁移前强制备份，失败可回滚到备份点。
//! 4. 数据库版本高于程序版本时拒绝运行（不猜测、不降级、不丢数据）。

use std::path::Path;

use rusqlite::{params, Connection, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::database::{backup, now_iso};
use crate::error::{AppError, AppResult};

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "0001_init",
        sql: include_str!("migrations/0001_init.sql"),
    },
    Migration {
        version: 2,
        name: "0002_model_alignment",
        sql: include_str!("migrations/0002_model_alignment.sql"),
    },
];

fn checksum(sql: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(sql.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn code_version() -> i64 {
    MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
}

fn ensure_table(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           version    INTEGER PRIMARY KEY,
           name       TEXT NOT NULL,
           checksum   TEXT NOT NULL,
           applied_at TEXT NOT NULL
         );",
    )?;
    Ok(())
}

fn applied(conn: &Connection) -> AppResult<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT version, checksum FROM schema_migrations ORDER BY version")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn run(conn: &mut Connection, db_path: &Path) -> AppResult<()> {
    ensure_table(conn)?;
    let applied = applied(conn)?;

    let db_version = applied.iter().map(|(v, _)| *v).max().unwrap_or(0);
    if db_version > code_version() {
        return Err(AppError::database_too_new(db_version, code_version()));
    }

    for migration in MIGRATIONS {
        if let Some((_, recorded)) = applied.iter().find(|(v, _)| *v == migration.version) {
            if recorded != &checksum(migration.sql) {
                return Err(AppError::internal(format!(
                    "迁移脚本 {} 在应用后被修改（校验和不匹配），请恢复原脚本或新增一条迁移",
                    migration.name
                )));
            }
        }
    }

    let pending: Vec<&Migration> = MIGRATIONS
        .iter()
        .filter(|m| !applied.iter().any(|(v, _)| *v == m.version))
        .collect();

    if pending.is_empty() {
        return Ok(());
    }

    // 已有数据才需要备份（首次建库没有可丢的东西）
    if !applied.is_empty() && db_path.to_string_lossy() != ":memory:" {
        backup::snapshot(conn, db_path, "pre-migrate")?;
    }

    for migration in pending {
        apply_one(conn, migration)?;
    }

    Ok(())
}

fn apply_one(conn: &mut Connection, migration: &Migration) -> AppResult<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(migration.sql)?;
    tx.execute(
        "INSERT INTO schema_migrations(version, name, checksum, applied_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            migration.version,
            migration.name,
            checksum(migration.sql),
            now_iso()
        ],
    )?;
    tx.pragma_update(None, "user_version", migration.version)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::path::PathBuf;

    fn temp_db(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sage-migrate-{tag}-{}-{}.db",
            std::process::id(),
            now_iso().replace([':', '.'], "-")
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn cleanup(path: &Path) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    fn reopen_error(path: &Path) -> String {
        match Database::open(path) {
            Ok(_) => panic!("应当拒绝打开"),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn rejects_database_from_newer_app() {
        let path = temp_db("newer");
        {
            let db = Database::open(&path).expect("first open");
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT INTO schema_migrations(version, name, checksum, applied_at)
                     VALUES (999, '0999_future', 'x', '2026-01-01T00:00:00.000Z')",
                    [],
                )?;
                Ok(())
            })
            .expect("insert future migration");
        }

        let message = reopen_error(&path);
        assert!(message.contains("高于当前程序"), "未识别未来版本：{message}");
        cleanup(&path);
    }

    #[test]
    fn detects_tampered_migration_script() {
        let path = temp_db("tampered");
        {
            let db = Database::open(&path).expect("first open");
            db.with_conn(|conn| {
                conn.execute(
                    "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 1",
                    [],
                )?;
                Ok(())
            })
            .expect("tamper");
        }

        let message = reopen_error(&path);
        assert!(message.contains("校验和"), "未识别脚本被改动：{message}");
        cleanup(&path);
    }

    /// 生产库都停留在 v1：必须验证"升级不丢数据"这条真实路径。
    #[test]
    fn upgrades_v1_database_and_keeps_data() {
        let path = temp_db("upgrade");
        {
            let conn = Connection::open(&path).expect("open raw");
            conn.execute_batch(MIGRATIONS[0].sql).expect("apply v1");
            ensure_table(&conn).expect("meta");
            conn.execute(
                "INSERT INTO schema_migrations(version, name, checksum, applied_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![1_i64, "0001_init", checksum(MIGRATIONS[0].sql), now_iso()],
            )
            .expect("record v1");
            conn.pragma_update(None, "user_version", 1_i64)
                .expect("user_version");

            // 造一条"已完成"的计划项：验证 DROP COLUMN 不会丢行、且折进 status
            conn.execute_batch(
                "INSERT INTO study_plans (id, revision, status, generated_by, generated_at, model,
                                          difficulty_current, difficulty_suggested,
                                          difficulty_reason, created_at)
                 VALUES ('plan_1', 1, 'active', 'seed', '2026-01-01T00:00:00.000Z', 'test',
                         3, 3, '', '2026-01-01T00:00:00.000Z');
                 INSERT INTO plan_days (id, plan_id, date, focus)
                 VALUES ('day_1', 'plan_1', '2026-01-01', 'x');
                 INSERT INTO plan_items (id, plan_day_id, sort_order, title, activity,
                                         minutes, xp, reason, done, status, action_type)
                 VALUES ('item_1', 'day_1', 0, 't', 'review', 5, 0, '', 1, 'pending', 'review');",
            )
            .expect("seed plan");
        }

        let db = Database::open(&path).expect("upgrade");
        db.with_conn(|conn| {
            let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            assert_eq!(version, crate::database::SCHEMA_VERSION);

            let (rows, status): (i64, String) = conn.query_row(
                "SELECT COUNT(*), MAX(status) FROM plan_items",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            assert_eq!(rows, 1, "迁移丢了计划项");
            assert_eq!(status, "done", "done 未折进 status");

            // 汇总分项由内容包显式标记，不再依赖"有没有题"隐式推断
            let role: String = conn.query_row(
                "SELECT section_role FROM assessment_sections WHERE id = 'sec_speak_eval'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(role, "summary");

            let graded: i64 = conn.query_row(
                "SELECT COUNT(*) FROM assessment_sections WHERE section_role = 'graded'",
                [],
                |row| row.get(0),
            )?;
            assert!(graded > 0, "可作答分项应保持 graded");

            let fks: i64 = conn.query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_list('training_answers')
                  WHERE \"table\" = 'exercises'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(fks, 1, "training_answers.exercise_id 缺少外键");
            Ok(())
        })
        .expect("inspect");

        cleanup(&path);
    }

    #[test]
    fn applies_migrations_in_order_on_empty_database() {
        let path = temp_db("fresh");
        {
            let db = Database::open(&path).expect("open");
            db.with_conn(|conn| {
                let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
                assert_eq!(version, code_version());
                assert_eq!(applied(conn)?.len(), MIGRATIONS.len());
                Ok(())
            })
            .expect("inspect");
        }
        cleanup(&path);
    }
}
