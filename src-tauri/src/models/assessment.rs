//! 测评模块与结果。对应 `src/types/assessment.ts`。

use serde::{Deserialize, Serialize};

use super::exercise::Exercise;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessmentSection {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub skill: String,
    pub items: Vec<Exercise>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastAttempt {
    pub at: String,
    pub score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessmentModule {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub minutes: i64,
    pub sections: Vec<AssessmentSection>,
    pub last_attempt: Option<LastAttempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionOutcome {
    pub section_id: String,
    pub title: String,
    pub correct: i64,
    pub total: i64,
    pub score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowUp {
    pub label: String,
    pub kind: String,
    pub track: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessmentOutcome {
    pub module_id: String,
    pub score: i64,
    pub accuracy: i64,
    pub level: String,
    pub per_section: Vec<SectionOutcome>,
    pub strengths: Vec<String>,
    pub weaknesses: Vec<String>,
    pub recommendation: String,
    pub finished_at: String,
    pub follow_up: Option<FollowUp>,
    /// 是否为重复提交（命中 `submission_id` 幂等键，未产生新记录）
    #[serde(default)]
    pub duplicate: bool,
}
