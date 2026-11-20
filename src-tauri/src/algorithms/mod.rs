//! 确定性算法（纯函数、零 IO）。
//!
//! 这里的实现与 `src/lib/algorithms/*` 一一对应：TS 版本保留为 mock 实现与对照基准，
//! Rust 版本是 backend 模式下的权威实现。两侧一致性由 `tests/fixtures/parity` 守住。

pub mod ability;
pub mod grading;
pub mod layout;
pub mod mastery;
pub mod selection;
pub mod srs;
pub mod util;
pub mod xp;
