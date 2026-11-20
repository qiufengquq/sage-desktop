//! 判分规则（对照组：`src/lib/grading.ts`）。
//!
//! backend 模式下由 Rust 裁定得分：前端提交的 `correct/partialScore` 一律忽略，
//! 避免"改前端就能刷分"。规则必须与 TS 逐行等价，由 parity 测试守住。

use serde_json::{Map, Value as Json};

use super::util::{character_similarity, normalize_answer, similarity};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreOutcome {
    pub correct: bool,
    pub partial: f64,
}

impl ScoreOutcome {
    fn full(correct: bool) -> Self {
        Self { correct, partial: if correct { 1.0 } else { 0.0 } }
    }
}

/// JS `String(value ?? "")` 的近似实现
fn as_text(value: &Json) -> String {
    match value {
        Json::Null => String::new(),
        Json::String(text) => text.clone(),
        Json::Bool(flag) => flag.to_string(),
        Json::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

/// 仅当是普通对象时返回映射（数组不算），对应 TS 里的 `typeof value === "object" && !Array.isArray(value)`
fn as_mapping(value: &Json) -> Option<&Map<String, Json>> {
    match value {
        Json::Object(map) => Some(map),
        _ => None,
    }
}

fn mapping_text(map: &Map<String, Json>, key: &str) -> String {
    map.get(key).map(as_text).unwrap_or_default()
}

fn options_of(payload: &Json) -> Vec<(String, String, bool)> {
    payload
        .get("options")
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    (
                        item.get("id").map(as_text).unwrap_or_default(),
                        item.get("label").map(as_text).unwrap_or_default(),
                        item.get("correct").and_then(Json::as_bool).unwrap_or(false),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn answer_text(payload: &Json) -> String {
    payload.get("answer").map(as_text).unwrap_or_default()
}

fn references_of(payload: &Json) -> Vec<String> {
    payload
        .get("references")
        .and_then(Json::as_array)
        .map(|items| items.iter().map(as_text).collect())
        .unwrap_or_default()
}

fn answer_list(payload: &Json) -> Vec<String> {
    payload
        .get("answer")
        .and_then(Json::as_array)
        .map(|items| items.iter().map(as_text).collect())
        .unwrap_or_default()
}

pub fn score_answer(kind: &str, payload: &Json, value: &Json) -> ScoreOutcome {
    match kind {
        "word-match" => {
            let mapping = as_mapping(value).cloned().unwrap_or_default();
            let pairs: Vec<(String, String)> = payload
                .get("pairs")
                .and_then(Json::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| {
                            (
                                item.get("left").map(as_text).unwrap_or_default(),
                                item.get("right").map(as_text).unwrap_or_default(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();

            let total = pairs.len();
            let hits = pairs
                .iter()
                .filter(|(left, right)| &mapping_text(&mapping, left) == right)
                .count();

            ScoreOutcome {
                correct: total == 0 || hits == total,
                partial: if total == 0 { 0.0 } else { hits as f64 / total as f64 },
            }
        }

        "multiple-choice" => {
            let chosen = as_text(value);
            let hit = options_of(payload)
                .into_iter()
                .find(|(id, label, _)| id == &chosen || label == &chosen)
                .map(|(_, _, correct)| correct)
                .unwrap_or(false);
            ScoreOutcome::full(hit)
        }

        "cloze" => {
            let chosen = as_text(value);
            let by_option = options_of(payload)
                .into_iter()
                .find(|(id, label, _)| id == &chosen || label == &chosen)
                .map(|(_, _, correct)| correct)
                .unwrap_or(false);
            let correct =
                by_option || normalize_answer(&chosen) == normalize_answer(&answer_text(payload));
            ScoreOutcome::full(correct)
        }

        "spelling" | "dictation" => {
            let typed = normalize_answer(&as_text(value));
            let answer = answer_text(payload);
            let correct = typed == normalize_answer(&answer);
            ScoreOutcome {
                correct,
                partial: if correct {
                    1.0
                } else {
                    character_similarity(&as_text(value), &answer) * 0.6
                },
            }
        }

        "translate" | "rewrite" => {
            let typed = as_text(value);
            let mut candidates = vec![answer_text(payload)];
            candidates.extend(references_of(payload));

            let best = candidates
                .iter()
                .map(|candidate| similarity(&typed, candidate))
                .fold(0.0_f64, f64::max);
            let exact = candidates
                .iter()
                .any(|candidate| normalize_answer(candidate) == normalize_answer(&typed));

            ScoreOutcome {
                correct: exact || best >= 0.85,
                partial: if exact { 1.0 } else { best },
            }
        }

        "sentence-order" => {
            let ordered = match value {
                Json::Array(items) => items.iter().map(as_text).collect::<Vec<_>>().join(" "),
                other => as_text(other),
            };
            let expected = answer_list(payload).join(" ");
            let correct = normalize_answer(&ordered) == normalize_answer(&expected);
            ScoreOutcome {
                correct,
                partial: if correct { 1.0 } else { similarity(&ordered, &expected) },
            }
        }

        "error-correction" => {
            let typed = as_text(value);
            let expected = answer_text(payload);
            let correct = normalize_answer(&typed) == normalize_answer(&expected);
            ScoreOutcome {
                correct,
                partial: if correct { 1.0 } else { similarity(&typed, &expected) },
            }
        }

        "shadowing" => {
            let typed = as_text(value);
            let target = payload.get("text").map(as_text).unwrap_or_default();
            let partial = similarity(&typed, &target);
            ScoreOutcome { correct: partial >= 0.8, partial }
        }

        "roleplay" => {
            let answers = as_mapping(value).cloned().unwrap_or_default();
            let user_turns: Vec<(String, Vec<String>)> = payload
                .get("turns")
                .and_then(Json::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter(|turn| turn.get("speaker").and_then(Json::as_str) == Some("user"))
                        .map(|turn| {
                            let id = turn.get("id").map(as_text).unwrap_or_default();
                            let expected = turn
                                .get("expected")
                                .and_then(Json::as_array)
                                .map(|list| list.iter().map(as_text).collect())
                                .unwrap_or_default();
                            (id, expected)
                        })
                        .collect()
                })
                .unwrap_or_default();

            let total = user_turns.len();
            let hits = user_turns
                .iter()
                .filter(|(id, expected)| {
                    let said = mapping_text(&answers, id);
                    if said.trim().is_empty() {
                        return false;
                    }
                    let normalized = normalize_answer(&said);
                    expected
                        .iter()
                        .any(|candidate| normalized.contains(&normalize_answer(candidate)))
                })
                .count();

            ScoreOutcome {
                correct: total == 0 || hits == total,
                partial: if total == 0 { 0.0 } else { hits as f64 / total as f64 },
            }
        }

        _ => ScoreOutcome { correct: false, partial: 0.0 },
    }
}

/// 供"逐题即时反馈"使用的可读答案（不参与判分，只用于回显）。
///
/// 为什么需要它：判分权收归后端后，客户端不再持有正确答案，
/// 因此"答完这题正确答案是什么"必须由服务端单独回传。
pub fn answer_of(kind: &str, payload: &Json) -> Json {
    match kind {
        "multiple-choice" | "cloze" => {
            let labels: Vec<Json> = payload
                .get("options")
                .and_then(Json::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter(|item| item.get("correct").and_then(Json::as_bool) == Some(true))
                        .map(|item| item.get("label").cloned().unwrap_or(Json::Null))
                        .collect()
                })
                .unwrap_or_default();

            match labels.len() {
                0 => payload.get("answer").cloned().unwrap_or(Json::Null),
                1 => labels.into_iter().next().unwrap_or(Json::Null),
                _ => Json::Array(labels),
            }
        }

        "spelling" | "dictation" | "error-correction" | "translate" | "rewrite" | "sentence-order" => {
            payload.get("answer").cloned().unwrap_or(Json::Null)
        }

        "word-match" => payload.get("pairs").cloned().unwrap_or(Json::Null),

        "shadowing" => payload.get("text").cloned().unwrap_or(Json::Null),

        "roleplay" => Json::Array(
            payload
                .get("turns")
                .and_then(Json::as_array)
                .map(|turns| {
                    turns
                        .iter()
                        .filter(|turn| turn.get("speaker").and_then(Json::as_str) == Some("user"))
                        .map(|turn| {
                            serde_json::json!({
                                "id": turn.get("id").cloned().unwrap_or(Json::Null),
                                "expected": turn.get("expected").cloned().unwrap_or(Json::Null),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),

        _ => Json::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn multiple_choice_matches_by_id_or_label() {
        let payload = json!({
            "options": [
                { "id": "a", "label": "苹果", "correct": true },
                { "id": "b", "label": "香蕉", "correct": false }
            ]
        });
        assert!(score_answer("multiple-choice", &payload, &json!("a")).correct);
        assert!(score_answer("multiple-choice", &payload, &json!("苹果")).correct);
        assert!(!score_answer("multiple-choice", &payload, &json!("b")).correct);
    }

    #[test]
    fn spelling_partial_credit_is_capped() {
        let payload = json!({ "answer": "apple" });
        let exact = score_answer("spelling", &payload, &json!("Apple"));
        assert!(exact.correct);
        assert_eq!(exact.partial, 1.0);

        let near = score_answer("spelling", &payload, &json!("appl"));
        assert!(!near.correct);
        assert!(near.partial > 0.0 && near.partial < 1.0);
    }

    #[test]
    fn sentence_order_compares_joined_text() {
        let payload = json!({ "answer": ["I", "like", "tea"] });
        assert!(score_answer("sentence-order", &payload, &json!(["I", "like", "tea"])).correct);
        let wrong = score_answer("sentence-order", &payload, &json!(["I", "tea", "like"]));
        assert!(!wrong.correct);
        assert!(wrong.partial > 0.0);
    }

    #[test]
    fn roleplay_requires_expected_phrases() {
        let payload = json!({
            "turns": [
                { "id": "t1", "speaker": "ai", "en": "Hi", "zh": "嗨" },
                { "id": "t2", "speaker": "user", "en": "Hi", "zh": "嗨", "expected": ["hello"] }
            ]
        });
        assert!(score_answer("roleplay", &payload, &json!({ "t2": "Hello there" })).correct);
        assert!(!score_answer("roleplay", &payload, &json!({ "t2": "  " })).correct);
        assert!(!score_answer("roleplay", &payload, &json!({ "t2": "nope" })).correct);
    }

    #[test]
    fn empty_answer_scores_zero() {
        for kind in ["spelling", "dictation", "shadowing", "error-correction"] {
            let payload = json!({ "answer": "abc", "text": "abc def" });
            let outcome = score_answer(kind, &payload, &json!(""));
            assert!(!outcome.correct, "{kind} 空答案不能判对");
            assert_eq!(outcome.partial, 0.0, "{kind} 空答案不能有保底分");
        }
    }
}
