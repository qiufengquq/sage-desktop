//! 构建脚本：注入 AI 框架的版本与构建信息。
//!
//! 两种仓库形态都要支持：
//!   1. **monorepo（当前）**：`crates/ai-core` 是 workspace 成员，`Cargo.lock` 在仓库根，
//!      依赖没有 `source` 字段，commit 需从 git 历史取；
//!   2. **独立仓库（历史形态）**：`ai-core` 是 git 依赖，`Cargo.lock` 的
//!      `source = "git+...#<sha>"` 直接给出 commit。
//!
//! 注入失败不 panic，但会写入 `unknown` / `workspace`，并由
//! `ai::version::tests` 断言拦截，避免"界面上显示一个假版本"。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    inject_build_info();
    tauri_build::build();
}

fn inject_build_info() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let root = workspace_root(&manifest_dir);
    let lock_path = find_lock(&manifest_dir, &root);

    println!("cargo:rerun-if-changed=build.rs");
    if let Some(lock) = &lock_path {
        println!("cargo:rerun-if-changed={}", lock.display());
    }
    println!("cargo:rerun-if-changed={}", root.join("crates/ai-core/Cargo.toml").display());

    let lock = lock_path
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();

    let version = find_package(&lock, "ai-core")
        .map(|(version, _)| version)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| manifest_version(&root).unwrap_or_default());

    let commit = resolve_commit(&root, &lock);

    println!("cargo:rustc-env=SAGE_AI_CORE_VERSION={}", fallback(&version, "unknown"));
    println!("cargo:rustc-env=SAGE_AI_CORE_COMMIT={}", fallback(&commit, "unknown"));
    println!("cargo:rustc-env=SAGE_BUILD_TIME={}", rfc3339_now());
    println!(
        "cargo:rustc-env=SAGE_BUILD_TARGET={}",
        std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string())
    );
}

/// monorepo 时 `Cargo.lock` 在 `src-tauri` 的上一层。
fn workspace_root(manifest_dir: &Path) -> PathBuf {
    match manifest_dir.parent() {
        Some(parent) if parent.join("Cargo.toml").exists() => parent.to_path_buf(),
        _ => manifest_dir.to_path_buf(),
    }
}

fn find_lock(manifest_dir: &Path, root: &Path) -> Option<PathBuf> {
    [manifest_dir.join("Cargo.lock"), root.join("Cargo.lock")]
        .into_iter()
        .find(|candidate| candidate.exists())
}

/// `crates/ai-core/Cargo.toml` 里的 `version`，作为 lock 缺失时的兜底。
fn manifest_version(root: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(root.join("crates/ai-core/Cargo.toml")).ok()?;
    raw.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("version = "))
        .map(|value| value.trim_matches('"').to_string())
}

fn fallback(value: &str, when_empty: &str) -> String {
    if value.is_empty() {
        when_empty.to_string()
    } else {
        value.to_string()
    }
}

/// 在 Cargo.lock 中按包名定位版本与 git commit（path 依赖的 commit 为空）。
fn find_package(lock: &str, name: &str) -> Option<(String, String)> {
    let target = format!("name = \"{name}\"");
    for block in lock.split("[[package]]") {
        if !block.lines().any(|line| line.trim() == target) {
            continue;
        }

        let mut version = String::new();
        let mut commit = String::new();
        for line in block.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("version = ") {
                version = rest.trim_matches('"').to_string();
            } else if let Some(rest) = line.strip_prefix("source = ") {
                let raw = rest.trim_matches('"');
                if let Some((_, hash)) = raw.rsplit_once('#') {
                    commit = hash.to_string();
                }
            }
        }

        if !version.is_empty() {
            return Some((version, commit));
        }
    }
    None
}

/// 解析"实际编译进来的框架代码"对应的 commit。
///
/// 优先级：git 依赖的 source（独立仓库形态）→ `crates/` 最后一次提交
/// （monorepo 形态，能精确定位框架代码范围）→ 仓库 HEAD → `workspace`。
fn resolve_commit(root: &Path, lock: &str) -> String {
    if let Some((_, commit)) = find_package(lock, "ai-core") {
        if !commit.is_empty() {
            return commit;
        }
    }

    if let Some(sha) = git(root, &["log", "-1", "--format=%H", "--", "crates"]) {
        let dirty = git(root, &["status", "--porcelain", "--", "crates"])
            .map(|status| !status.trim().is_empty())
            .unwrap_or(false);
        return if dirty { format!("{sha}-dirty") } else { sha };
    }

    git(root, &["rev-parse", "HEAD"]).unwrap_or_else(|| "workspace".to_string())
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// 不引入额外 build-dependency：用 UNIX 秒数生成 UTC RFC3339。
fn rfc3339_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant 的 days → (y, m, d) 算法（1970-01-01 起算）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}
