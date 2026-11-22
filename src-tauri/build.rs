//! 构建脚本：注入 AI 框架的版本与构建信息。
//!
//! 为什么从 `Cargo.lock` 读：版本号由 Cargo 解析，手写在代码里会漂移。
//! SAGE Intelligence 已并入本仓库（`crates/ai-core`、`crates/ai-core-tauri`），
//! 因此 lock 位于 workspace 根，且 `ai-core` 是 path 依赖、没有 `source = "git+..."`；
//! 此时 commit 记为 `local`，表示源码就在本仓库内。

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    inject_build_info();
    tauri_build::build();
}

fn inject_build_info() {
    // workspace 根在仓库根，lock 在 src-tauri 的上一级。
    println!("cargo:rerun-if-changed=../Cargo.lock");
    println!("cargo:rerun-if-changed=tauri.conf.json");

    let lock = std::fs::read_to_string("../Cargo.lock").unwrap_or_default();
    let (version, commit) = find_package(&lock, "ai-core").unwrap_or_default();

    println!("cargo:rustc-env=SAGE_AI_CORE_VERSION={}", fallback(&version, "unknown"));
    // path 依赖没有 git source，源码已内联在本仓库，用 `local` 标记。
    println!("cargo:rustc-env=SAGE_AI_CORE_COMMIT={}", fallback(&commit, "local"));
    println!("cargo:rustc-env=SAGE_BUILD_TIME={}", rfc3339_now());
    println!(
        "cargo:rustc-env=SAGE_BUILD_TARGET={}",
        std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string())
    );
}

fn fallback(value: &str, when_empty: &str) -> String {
    if value.is_empty() {
        when_empty.to_string()
    } else {
        value.to_string()
    }
}

/// 在 Cargo.lock 中按包名定位版本与 git commit。
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
