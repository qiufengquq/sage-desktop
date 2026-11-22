//! 版本信息：客户端版本、框架版本、框架 commit、集成协议版本。
//!
//! 客户端版本来自 `CARGO_PKG_VERSION`；框架版本与 commit 由 `build.rs`
//! 从 `Cargo.lock` 解析后注入，保证"显示的版本 == 实际编译进来的版本"。

use crate::models::ai::AiVersionInfo;

pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const INTELLIGENCE_VERSION: &str = env!("SAGE_AI_CORE_VERSION");
pub const INTELLIGENCE_COMMIT: &str = env!("SAGE_AI_CORE_COMMIT");
pub const BUILD_TIME: &str = env!("SAGE_BUILD_TIME");
pub const BUILD_TARGET: &str = env!("SAGE_BUILD_TARGET");

/// 客户端 ⇄ 框架的集成契约版本，与两个项目的 SemVer 无关。
/// 变更事件名、命令参数、错误码映射时递增（见 docs/AI_INTEGRATION.md）。
pub const PROTOCOL_VERSION: &str = "1";

/// 集成模式恒为 `library`：AI 与客户端编译进同一个可执行文件，运行在同一 Rust 进程内。
/// Sidecar / 独立 Runtime / 本地 HTTP 服务已被架构明确排除，不保留开关。
pub const INTEGRATION_MODE: &str = "library";

pub fn info() -> AiVersionInfo {
    AiVersionInfo {
        client_version: CLIENT_VERSION.to_string(),
        intelligence_version: INTELLIGENCE_VERSION.to_string(),
        intelligence_commit: INTELLIGENCE_COMMIT.to_string(),
        protocol_version: PROTOCOL_VERSION.to_string(),
        integration_mode: INTEGRATION_MODE.to_string(),
        build_time: BUILD_TIME.to_string(),
        platform: BUILD_TARGET.to_string(),
    }
}

/// 提交短 SHA（展示用），异常输入原样返回。
pub fn short_commit(commit: &str) -> String {
    if commit.len() >= 7 && commit.chars().all(|c| c.is_ascii_hexdigit()) {
        commit[..7].to_string()
    } else {
        commit.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这条测试守"版本可追溯"：注入失败会得到 `unknown`，必须让测试失败而不是静默降级。
    #[test]
    fn injected_versions_are_real() {
        assert_eq!(CLIENT_VERSION, "1.0.1");
        assert_ne!(INTELLIGENCE_VERSION, "unknown", "build.rs 没能从 Cargo.lock 读到 ai-core 版本");
        assert_ne!(INTELLIGENCE_COMMIT, "unknown", "build.rs 没能读到 git commit");
        assert_eq!(INTELLIGENCE_COMMIT.len(), 40, "commit 必须是完整 SHA");
        assert!(INTELLIGENCE_COMMIT.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(BUILD_TIME.ends_with('Z'), "构建时间必须是 UTC RFC3339");
        assert!(BUILD_TARGET.contains("windows"), "构建平台应反映真实目标");
        assert_eq!(PROTOCOL_VERSION, "1");
        assert_eq!(INTEGRATION_MODE, "library");
    }

    #[test]
    fn short_commit_truncates_valid_hash() {
        assert_eq!(short_commit("33be312c6949afa7f20051ec40fd520002ec5da6"), "33be312");
        assert_eq!(short_commit("unknown"), "unknown");
    }
}
