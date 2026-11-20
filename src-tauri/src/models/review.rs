//! 复习闭环的读写模型。对应 `src/types/review.ts`。
//!
//! 前端在 mock 模式下用 `selectors.ts::buildReviewCards` 现算卡片；
//! backend 模式下这份派生搬到这里，字段形状保持一致，页面不需要分支。

use serde::Serialize;

/// 复习队列中的一项：学习态 + 内容摘要 + 逾期信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewQueueItem {
    pub entity_id: String,
    pub kind: String,
    /// 主展示文本：单词 / 语法标题 / 英文句子
    pub label: String,
    /// 副展示文本：释义 / 英文名 / 中文翻译
    pub secondary: String,
    pub difficulty: i64,
    pub mastery: f64,
    pub strength: f64,
    pub state: String,
    pub due_at: String,
    /// 逾期自然日数，0 表示今天到期
    pub overdue_days: i64,
    pub last_reviewed_at: Option<String>,
    pub review_count: i64,
}

/// 四种自评分别会把下次复习排到多久之后，让用户做决定前就能看到
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPreview {
    pub again: String,
    pub hard: String,
    pub good: String,
    pub easy: String,
}

/// 复习卡片：先回忆，再揭晓
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCard {
    pub item: ReviewQueueItem,
    pub prompt: String,
    pub answer: String,
    pub detail: Vec<String>,
    pub focus: String,
    pub preview: ReviewPreview,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewQueuePayload {
    /// 当前所有到期条目数（可能大于 `cards.len()`）
    pub due_count: i64,
    pub cards: Vec<ReviewCard>,
}

/// 一次复习提交的结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSubmitResult {
    pub entity_id: String,
    pub rating: String,
    pub mastery_before: f64,
    pub mastery_after: f64,
    pub next_due_at: String,
    pub interval_days: i64,
    pub ease: f64,
    pub xp: i64,
    /// 本次调度的人类可读说明
    pub explanation: String,
}
