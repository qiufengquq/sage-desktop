//! 备份与恢复。
//!
//! 备份使用 SQLite 在线备份 API（`Backup`），而不是复制文件：
//! WAL 模式下直接拷 `.db` 可能拿到未合并的事务，`Backup` 会在读锁内给出一致快照。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Local;
use rusqlite::{Connection, OpenFlags};

use crate::database::SCHEMA_VERSION;
use crate::error::{AppError, AppResult};

/// 保留的备份份数（同一天最多 3 份，超出按时间淘汰）
const KEEP_TOTAL: usize = 10;
const KEEP_PER_DAY: usize = 3;

fn stem_of(db_path: &Path) -> String {
    db_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("sage")
        .to_string()
}

pub fn backup_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn new_name(db_path: &Path, tag: &str) -> String {
    format!(
        "{}-{}-{tag}.db",
        stem_of(db_path),
        Local::now().format("%Y%m%d-%H%M%S")
    )
}

/// 生成一份带标签的备份，并清理过期备份
pub fn snapshot(conn: &Connection, db_path: &Path, tag: &str) -> AppResult<PathBuf> {
    let dest = backup_dir(db_path).join(new_name(db_path, tag));
    copy_to(conn, &dest)?;
    prune(db_path)?;
    Ok(dest)
}

/// 把 `conn` 的内容一致地写入 `dest`
pub fn copy_to(conn: &Connection, dest: &Path) -> AppResult<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::internal(format!("创建备份目录失败：{e}")))?;
    }
    let mut target = Connection::open(dest)?;
    {
        let backup = rusqlite::backup::Backup::new(conn, &mut target)?;
        backup.run_to_completion(64, Duration::from_millis(10), None)?;
    }
    Ok(())
}

/// 只保留最近若干份备份
pub fn prune(db_path: &Path) -> AppResult<()> {
    let dir = backup_dir(db_path);
    let prefix = format!("{}-", stem_of(db_path));
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|e| AppError::internal(format!("读取备份目录失败：{e}")))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .map(|name| name.starts_with(&prefix) && name.ends_with(".db"))
                .unwrap_or(false)
        })
        .collect();

    // 文件名含时间戳，字典序倒序即"新 → 旧"
    files.sort();
    files.reverse();

    let mut per_day: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut kept = 0usize;
    for path in files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        // sage-20261114-213005-tag.db → 20261114
        let day = name
            .split('-')
            .nth(1)
            .unwrap_or("unknown")
            .to_string();
        let day_count = per_day.entry(day).or_insert(0);
        if kept >= KEEP_TOTAL || *day_count >= KEEP_PER_DAY {
            let _ = fs::remove_file(&path);
            continue;
        }
        *day_count += 1;
        kept += 1;
    }

    Ok(())
}

/// 校验一个备份文件是否可用于恢复（不复制，只检查）
pub fn validate_backup(path: &Path) -> AppResult<i64> {
    if !path.exists() {
        return Err(AppError::not_found(path.display()));
    }
    let check = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    let integrity: String = check.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(AppError::corrupt(format!("备份完整性检查失败：{integrity}")));
    }

    let version: i64 = check
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap_or(0);
    if version > SCHEMA_VERSION {
        return Err(AppError::database_too_new(version, SCHEMA_VERSION));
    }

    Ok(version)
}

/// 用备份文件覆盖主库。
///
/// 调用方必须先确保没有其它线程正在使用数据库（命令层会在恢复后要求重启应用）。
pub fn restore(db_path: &Path, source: &Path) -> AppResult<()> {
    let version = validate_backup(source)?;

    // 恢复前先备份当前库，避免选错文件后无法回头
    if db_path.exists() {
        if let Ok(conn) = Connection::open(db_path) {
            let _ = snapshot(&conn, db_path, "pre-restore");
        }
    }

    fs::copy(source, db_path).map_err(|e| AppError::internal(format!("复制备份文件失败：{e}")))?;

    // WAL/SHM 属于被替换掉的旧库，必须删除，否则会与新主库不一致
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{suffix}", db_path.display()));
    }

    let _ = version;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sage-backup-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn snapshot_produces_valid_database() {
        let dir = temp_dir("snapshot");
        let db_path = dir.join("sage.db");
        let db = Database::open(&db_path).expect("open");

        let dest = db
            .with_conn(|conn| snapshot(conn, &db_path, "test"))
            .expect("snapshot");

        assert!(dest.exists(), "备份文件未生成");
        let version = validate_backup(&dest).expect("validate");
        assert_eq!(version, SCHEMA_VERSION);

        // 备份里应当有同一套表
        let restored = Connection::open(&dest).expect("open backup");
        let tables: i64 = restored
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(tables, 36);

        drop(restored);
        drop(db);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_newest_backups() {
        let dir = temp_dir("prune");
        let db_path = dir.join("sage.db");
        let db = Database::open(&db_path).expect("open");

        // 直接造 15 个备份文件，验证清理后不超过上限
        for index in 0..15 {
            let path = dir.join(format!("sage-202601{:02}-1200{:02}-x.db", index + 1, index));
            fs::write(&path, b"fake").expect("write");
        }
        prune(&db_path).expect("prune");

        let remaining = fs::read_dir(&dir)
            .expect("read")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with("sage-") && n.ends_with(".db"))
                    .unwrap_or(false)
            })
            .count();
        assert!(remaining <= KEEP_TOTAL, "备份数量未收敛：{remaining}");

        drop(db);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_corrupted_backup() {
        let dir = temp_dir("corrupt");
        let broken = dir.join("broken.db");
        fs::write(&broken, b"this is not a database").expect("write");

        let outcome = validate_backup(&broken);
        assert!(outcome.is_err(), "损坏文件不应通过校验");

        let _ = fs::remove_dir_all(&dir);
    }
}
