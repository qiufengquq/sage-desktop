//! 仓储层：只负责 SQL，不含业务规则，不管理事务。
//!
//! 所有函数接收 `&Connection`，因此既能在只读查询里用，也能在 `Transaction` 内复用
//! （`Transaction` 通过 Deref 提供 `&Connection`）。

pub mod assessment_repository;
pub mod entity_repository;
pub mod exercise_repository;
pub mod mastery_repository;
pub mod review_repository;
pub mod state_repository;
pub mod training_repository;
pub mod user_repository;
