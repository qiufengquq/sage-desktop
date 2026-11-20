//! 题目、训练会话与训练结果。对应 `src/types/exercise.ts`。
//!
//! `payload` 用 `#[serde(flatten)]` 承载各题型专有字段，序列化时直接摊平到对象顶层，
//! 因此前端拿到的 `Exercise` 与 TS 的可辨识联合完全同形。

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExercisePrompt {
    pub instruction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exercise {
    pub id: String,
    pub kind: String,
    pub difficulty: i64,
    pub xp: i64,
    pub prompt: ExercisePrompt,
    pub entity_ids: Vec<String>,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(flatten)]
    pub payload: Json,
}

impl Exercise {
    /// 题型专有字段（判分时读取正确答案用）
    pub fn payload(&self) -> &Json {
        &self.payload
    }

    /// 转成"可以下发给客户端"的形态：**移除一切答案字段**。
    ///
    /// 为什么需要：判分与逐题反馈都在服务端（`training_check` / `assessment_check`），
    /// 客户端没有任何理由持有正确答案。留着就等于把题库答案直接送到前端。
    ///
    /// 两类特殊题型：
    /// - `word-match` 的 `pairs` 本身既是素材又是答案 → 拆成 `lefts` / `rights`；
    /// - `sentence-order` 的 `answer` 是答案，词块在 `tokens` 里，可直接删。
    pub fn to_player(&self) -> Exercise {
        let mut payload = self.payload.clone();
        let Some(map) = payload.as_object_mut() else {
            return Exercise {
                payload,
                ..self.clone()
            };
        };

        match self.kind.as_str() {
            "word-match" => {
                let pairs = map.remove("pairs").unwrap_or(Json::Null);
                let (lefts, rights) = split_pairs(&pairs);
                map.insert("lefts".into(), Json::Array(lefts));
                map.insert("rights".into(), Json::Array(rights));
            }

            "multiple-choice" | "cloze" => {
                strip_option_flags(map);
                if self.kind == "cloze" {
                    map.remove("answer");
                }
            }

            "spelling" | "dictation" | "translate" | "rewrite" | "error-correction"
            | "sentence-order" => {
                map.remove("answer");
                map.remove("references");
            }

            "roleplay" => {
                if let Some(turns) = map.get_mut("turns").and_then(Json::as_array_mut) {
                    for turn in turns {
                        if let Some(object) = turn.as_object_mut() {
                            object.remove("expected");
                        }
                    }
                }
            }

            // shadowing 的 text 既是朗读素材也是判分目标，无法隐藏，属已知限制
            _ => {}
        }

        Exercise {
            payload,
            ..self.clone()
        }
    }
}

fn split_pairs(pairs: &Json) -> (Vec<Json>, Vec<Json>) {
    let items = pairs.as_array().cloned().unwrap_or_default();
    let lefts: Vec<Json> = items
        .iter()
        .map(|item| item.get("left").cloned().unwrap_or(Json::Null))
        .collect();
    let rights: Vec<Json> = items
        .iter()
        .map(|item| item.get("right").cloned().unwrap_or(Json::Null))
        .collect();
    (lefts, rights)
}

fn strip_option_flags(map: &mut serde_json::Map<String, Json>) {
    if let Some(options) = map.get_mut("options").and_then(Json::as_array_mut) {
        for option in options {
            if let Some(object) = option.as_object_mut() {
                object.remove("correct");
            }
        }
    }
}

/// 逐题即时反馈的结果。
///
/// 判分只在服务端发生；`correct_answer` 是**回显用**的可读答案，
/// 客户端拿到的题目载荷本身不含答案。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerCheck {
    pub exercise_id: String,
    pub correct: bool,
    pub partial_score: f64,
    /// 字符串 / 数组 / 配对列表，按题型而定
    pub correct_answer: Json,
    #[serde(default)]
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingSession {
    pub id: String,
    pub track: String,
    pub kind: String,
    pub title: String,
    pub exercises: Vec<Exercise>,
    pub started_at: String,
    pub estimated_minutes: i64,
    pub target_entity_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseAnswer {
    pub exercise_id: String,
    /// 字符串答案、排序数组或连线映射
    pub value: Json,
    #[serde(default)]
    pub correct: bool,
    #[serde(default)]
    pub partial_score: f64,
    #[serde(default)]
    pub elapsed_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindScore {
    pub kind: String,
    pub correct: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseResult {
    pub session_id: String,
    pub track: String,
    pub answers: Vec<ExerciseAnswer>,
    pub score: f64,
    pub max_score: i64,
    pub accuracy: i64,
    pub duration_ms: i64,
    pub xp: i64,
    pub per_kind: Vec<KindScore>,
    pub weak_entity_ids: Vec<String>,
    pub finished_at: String,
    /// 是否为重复提交（同一 `session_id` 已计过分）
    #[serde(default)]
    pub duplicate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingOption {
    pub id: String,
    pub track: String,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub icon: String,
    pub minutes: i64,
    pub item_count: i64,
    pub difficulty: i64,
    /// 已有进度（0–1）；当前按"该题型已作答题目占比"计算
    pub progress: f64,
}
