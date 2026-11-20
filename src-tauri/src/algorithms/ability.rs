//! 能力画像（对照组：`src/lib/algorithms/ability.ts`）。
//!
//! 纯确定性规则，不模拟 AI。六个轴的数据来源必须如实标注：
//! - `measured`：有该模块的测评记录，分数主要来自测评
//! - `estimated`：没有直接测评，由训练表现或基线推算

use std::collections::HashMap;

use serde::Serialize;

use crate::models::state::EntityState;

use super::util::clamp;

pub const AXIS_KEYS: [&str; 6] = [
    "vocabulary",
    "grammar",
    "listening",
    "speaking",
    "reading",
    "writing",
];

fn axis_label(key: &str) -> &'static str {
    match key {
        "vocabulary" => "词汇量",
        "grammar" => "语法准确度",
        "listening" => "听力理解",
        "speaking" => "口语表达",
        "reading" => "阅读理解",
        _ => "写作",
    }
}

fn benchmark_of(key: &str) -> i64 {
    match key {
        "vocabulary" => 60,
        "grammar" => 58,
        "listening" => 62,
        "speaking" => 55,
        "reading" => 64,
        _ => 56,
    }
}

fn overall_weight(key: &str) -> f64 {
    match key {
        "vocabulary" => 0.2,
        "grammar" => 0.2,
        "listening" => 0.15,
        "speaking" => 0.2,
        "reading" => 0.15,
        _ => 0.1,
    }
}

fn fallback_of(key: &str) -> i64 {
    match key {
        "vocabulary" => 62,
        "grammar" => 54,
        "listening" => 58,
        "speaking" => 47,
        "reading" => 66,
        _ => 52,
    }
}

pub fn level_of_score(score: i64) -> &'static str {
    match score {
        value if value >= 88 => "C1",
        value if value >= 75 => "B2",
        value if value >= 60 => "B1",
        value if value >= 45 => "A2",
        _ => "A1",
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AbilityAxis {
    pub key: String,
    pub label: String,
    pub score: i64,
    pub benchmark: i64,
    pub level: String,
    pub confidence: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AbilityProfile {
    pub level: String,
    pub target_level: String,
    pub overall: i64,
    pub axes: Vec<AbilityAxis>,
    pub strengths: Vec<String>,
    pub weaknesses: Vec<String>,
    pub updated_at: String,
    pub source_assessment_id: Option<String>,
}

/// 一次测评的最小信息（用于取"最新一次"）
#[derive(Debug, Clone)]
pub struct AssessmentSummary {
    pub id: String,
    pub module_id: String,
    pub score: i64,
    pub finished_at: String,
}

pub struct AbilityInput<'a> {
    pub target_level: &'a str,
    pub assessments: &'a [AssessmentSummary],
    /// 已按题型聚合的训练表现：(kind, correct, total)
    pub kind_totals: &'a [(String, i64, i64)],
    pub states: &'a [EntityState],
    pub now_iso: &'a str,
}

#[derive(Debug, Clone, Copy)]
struct Accuracy {
    value: Option<i64>,
    items: i64,
}

pub fn build_ability_profile(input: AbilityInput<'_>) -> AbilityProfile {
    let latest_of = |module_id: &str| -> Option<&AssessmentSummary> {
        input
            .assessments
            .iter()
            .filter(|attempt| attempt.module_id == module_id)
            .max_by(|left, right| {
                left.finished_at
                    .cmp(&right.finished_at)
                    .then(left.id.cmp(&right.id))
            })
    };

    let accuracy_of_kinds = |kinds: &[&str]| -> Accuracy {
        let mut correct = 0i64;
        let mut total = 0i64;
        for (kind, hit, count) in input.kind_totals {
            if !kinds.contains(&kind.as_str()) {
                continue;
            }
            correct += hit;
            total += count;
        }
        Accuracy {
            value: if total == 0 {
                None
            } else {
                Some(((correct as f64 / total as f64) * 100.0).round() as i64)
            },
            items: total,
        }
    };

    let avg_mastery = |kinds: &[&str]| -> Option<i64> {
        let values: Vec<f64> = input
            .states
            .iter()
            .filter(|state| kinds.contains(&state.kind.as_str()))
            .map(|state| state.mastery)
            .collect();
        if values.is_empty() {
            return None;
        }
        Some((values.iter().sum::<f64>() / values.len() as f64).round() as i64)
    };

    let vocab_assessment = latest_of("vocabulary");
    let grammar_assessment = latest_of("grammar");
    let speaking_assessment = latest_of("speaking");

    let word_mastery = avg_mastery(&["word", "phrase"]);
    let grammar_mastery = avg_mastery(&["grammar"]);
    let listening = accuracy_of_kinds(&["dictation", "shadowing"]);
    let reading = accuracy_of_kinds(&["cloze", "translate"]);
    let writing = accuracy_of_kinds(&["rewrite", "translate"]);
    let speaking_accuracy = accuracy_of_kinds(&["shadowing", "roleplay"]);

    let axes = vec![
        build_axis(
            "vocabulary",
            vocab_assessment.map(|item| item.score),
            word_mastery,
            "词汇测评",
            "词条平均掌握度",
        ),
        build_axis(
            "grammar",
            grammar_assessment.map(|item| item.score),
            grammar_mastery,
            "语法测评",
            "语法点平均掌握度",
        ),
        build_axis(
            "speaking",
            speaking_assessment.map(|item| item.score),
            speaking_accuracy.value,
            "口语测评",
            "口语训练正确率",
        ),
        build_axis_from_accuracy("listening", listening, "听写 / 跟读题正确率"),
        build_axis_from_accuracy("reading", reading, "完形 / 翻译题正确率"),
        build_axis_from_accuracy("writing", writing, "改写 / 翻译题正确率"),
    ];

    let overall = axes
        .iter()
        .map(|axis| axis.score as f64 * overall_weight(&axis.key))
        .sum::<f64>()
        .round() as i64;

    let mut ranked = axes.clone();
    ranked.sort_by(|left, right| {
        let left_margin = left.score - left.benchmark;
        let right_margin = right.score - right.benchmark;
        right_margin.cmp(&left_margin)
    });

    let mut strengths: Vec<String> = ranked
        .iter()
        .filter(|axis| axis.score >= axis.benchmark)
        .take(2)
        .map(|axis| {
            format!(
                "{} {} 分，高于参考均值 {} 分",
                axis.label,
                axis.score,
                axis.score - axis.benchmark
            )
        })
        .collect();

    let mut weaknesses: Vec<String> = ranked
        .iter()
        .filter(|axis| axis.score < axis.benchmark)
        .rev()
        .take(2)
        .map(|axis| {
            format!(
                "{} {} 分，低于参考均值 {} 分（{}）",
                axis.label,
                axis.score,
                axis.benchmark - axis.score,
                if axis.confidence == "measured" {
                    "测评结果"
                } else {
                    "由训练表现估算"
                }
            )
        })
        .collect();

    fill_to(&mut strengths, "暂无高于参考均值的维度，先完成一次测评");
    fill_to(&mut weaknesses, "各维度接近参考均值，可以开始提高难度");

    AbilityProfile {
        level: level_of_score(overall).to_string(),
        target_level: input.target_level.to_string(),
        overall,
        axes,
        strengths,
        weaknesses,
        updated_at: input.now_iso.to_string(),
        source_assessment_id: vocab_assessment
            .or(grammar_assessment)
            .or(speaking_assessment)
            .map(|item| item.id.clone()),
    }
}

fn build_axis(
    key: &str,
    assessment: Option<i64>,
    secondary: Option<i64>,
    assessment_label: &str,
    secondary_label: &str,
) -> AbilityAxis {
    match (assessment, secondary) {
        (Some(primary), Some(other)) => {
            let score = (primary as f64 * 0.6 + other as f64 * 0.4).round() as i64;
            axis_of(
                key,
                score,
                "measured",
                &format!(
                    "{assessment_label} {primary} 分 × 60% + {secondary_label} {other} × 40%"
                ),
            )
        }
        (Some(primary), None) => axis_of(
            key,
            primary,
            "measured",
            &format!("{assessment_label} {primary} 分"),
        ),
        (None, Some(other)) => axis_of(
            key,
            other,
            "estimated",
            &format!("{secondary_label} {other}（尚无测评）"),
        ),
        (None, None) => axis_of(key, fallback_of(key), "estimated", "基线估值，等待首次测评"),
    }
}

fn build_axis_from_accuracy(key: &str, accuracy: Accuracy, label: &str) -> AbilityAxis {
    match accuracy.value {
        Some(value) if accuracy.items >= 4 => axis_of(
            key,
            value,
            "estimated",
            &format!("{label} {value}%（{} 题）", accuracy.items),
        ),
        _ => axis_of(
            key,
            fallback_of(key),
            "estimated",
            &format!("基线估值（{label}样本不足）"),
        ),
    }
}

fn axis_of(key: &str, score: i64, confidence: &str, source: &str) -> AbilityAxis {
    let clamped = clamp(score as f64, 0.0, 100.0).round() as i64;
    AbilityAxis {
        key: key.to_string(),
        label: axis_label(key).to_string(),
        score: clamped,
        benchmark: benchmark_of(key),
        level: level_of_score(clamped).to_string(),
        confidence: confidence.to_string(),
        source: source.to_string(),
    }
}

fn fill_to(items: &mut Vec<String>, placeholder: &str) {
    while items.len() < 2 {
        items.push(placeholder.to_string());
    }
    items.truncate(2);
}

/// 把训练记录按题型聚合成 `kind_totals`
pub fn aggregate_kind_totals(rows: &[(String, i64, i64)]) -> Vec<(String, i64, i64)> {
    let mut map: HashMap<String, (i64, i64)> = HashMap::new();
    for (kind, correct, total) in rows {
        let entry = map.entry(kind.clone()).or_insert((0, 0));
        entry.0 += correct;
        entry.1 += total;
    }
    map.into_iter()
        .map(|(kind, (correct, total))| (kind, correct, total))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(id: &str, kind: &str, mastery: f64) -> EntityState {
        let mut value = EntityState::new(id, kind, "2026-11-14T00:00:00.000Z");
        value.mastery = mastery;
        value
    }

    #[test]
    fn levels_follow_cefr_steps() {
        assert_eq!(level_of_score(89), "C1");
        assert_eq!(level_of_score(88), "C1");
        assert_eq!(level_of_score(70), "B1");
        assert_eq!(level_of_score(44), "A1");
    }

    #[test]
    fn empty_data_falls_back_to_baseline() {
        let profile = build_ability_profile(AbilityInput {
            target_level: "B2",
            assessments: &[],
            kind_totals: &[],
            states: &[],
            now_iso: "2026-11-14T00:00:00.000Z",
        });

        assert_eq!(profile.axes.len(), 6);
        assert!(profile.axes.iter().all(|axis| axis.confidence == "estimated"));
        assert_eq!(profile.axes[0].score, 62);
        assert_eq!(profile.strengths.len(), 2);
        assert_eq!(profile.weaknesses.len(), 2);
        assert!(profile.source_assessment_id.is_none());
    }

    #[test]
    fn assessment_and_mastery_are_blended() {
        let assessments = vec![AssessmentSummary {
            id: "aa_1".to_string(),
            module_id: "vocabulary".to_string(),
            score: 80,
            finished_at: "2026-11-10T00:00:00.000Z".to_string(),
        }];
        let states = vec![state("w_1", "word", 60.0)];

        let profile = build_ability_profile(AbilityInput {
            target_level: "B2",
            assessments: &assessments,
            kind_totals: &[],
            states: &states,
            now_iso: "2026-11-14T00:00:00.000Z",
        });

        let vocabulary = &profile.axes[0];
        assert_eq!(vocabulary.confidence, "measured");
        assert_eq!(vocabulary.score, 72, "80×0.6 + 60×0.4 = 72");
        assert_eq!(profile.source_assessment_id.as_deref(), Some("aa_1"));
    }

    #[test]
    fn kind_accuracy_needs_enough_samples() {
        let few = vec![("dictation".to_string(), 2, 2)];
        let profile = build_ability_profile(AbilityInput {
            target_level: "B2",
            assessments: &[],
            kind_totals: &few,
            states: &[],
            now_iso: "2026-11-14T00:00:00.000Z",
        });
        let listening = profile
            .axes
            .iter()
            .find(|axis| axis.key == "listening")
            .expect("listening");
        assert_eq!(listening.score, 58, "样本不足时使用基线");
        assert!(listening.source.contains("样本不足"));

        let enough = vec![("dictation".to_string(), 12, 14), ("shadowing".to_string(), 8, 10)];
        let profile = build_ability_profile(AbilityInput {
            target_level: "B2",
            assessments: &[],
            kind_totals: &enough,
            states: &[],
            now_iso: "2026-11-14T00:00:00.000Z",
        });
        let listening = profile
            .axes
            .iter()
            .find(|axis| axis.key == "listening")
            .expect("listening");
        assert_eq!(listening.score, 83, "20/24 → 83");
    }
}
