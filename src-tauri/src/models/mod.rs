//! 与前端 TS 类型一一对应的数据结构。
//!
//! 命名约定：字段用 snake_case，序列化输出统一 `#[serde(rename_all = "camelCase")]`，
//! 保证前端拿到的 JSON 与 `src/types/*.ts` 完全一致（不需要前端做任何转换）。

pub mod assessment;
pub mod exercise;
pub mod knowledge;
pub mod review;
pub mod state;
pub mod user;
