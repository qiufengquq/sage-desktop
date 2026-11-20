//! 双端一致性测试。
//!
//! 用例由 `scripts/gen-parity.mjs` 用 TS 实现（对照基准）生成，
//! 这里逐条验证 Rust 实现。只要两侧行为分叉，本测试立即失败。

use std::collections::HashMap;

use chrono::DateTime;
use sage_lib::algorithms::{grading, mastery, selection, srs, xp};
use sage_lib::models::exercise::{Exercise, ExercisePrompt};
use serde_json::{Map, Value as Json};

const FIXTURE: &str = include_str!("../../tests/fixtures/parity/algorithms.json");

fn fixture() -> Json {
    serde_json::from_str(FIXTURE).expect("对照用例文件必须是合法 JSON")
}

fn ms(iso: &str) -> i64 {
    DateTime::parse_from_rfc3339(iso)
        .expect("时间格式")
        .timestamp_millis()
}

fn approx(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{label} 不一致：rust={actual} ts={expected}"
    );
}

fn num(value: &Json, key: &str) -> f64 {
    value.get(key).and_then(Json::as_f64).unwrap_or_else(|| panic!("缺少数字字段 {key}"))
}

fn int(value: &Json, key: &str) -> i64 {
    value
        .get(key)
        .and_then(Json::as_i64)
        .unwrap_or_else(|| panic!("缺少整数字段 {key}"))
}

#[test]
fn grading_matches_typescript() {
    let data = fixture();
    let cases = data["grading"].as_array().expect("grading 数组");

    for (index, case) in cases.iter().enumerate() {
        let kind = case["kind"].as_str().expect("kind");
        let payload = &case["payload"];
        let value = &case["value"];
        let expect = &case["expect"];

        let actual = grading::score_answer(kind, payload, value);
        let label = format!("grading[{index}] {kind}");

        assert_eq!(
            actual.correct,
            expect["correct"].as_bool().expect("correct"),
            "{label} 的 correct 不一致"
        );
        approx(actual.partial, num(expect, "partial"), &format!("{label}.partial"));
    }
}

#[test]
fn mastery_matches_typescript() {
    let data = fixture();
    let cases = data["mastery"].as_array().expect("mastery 数组");

    for (index, case) in cases.iter().enumerate() {
        let snapshot = &case["snapshot"];
        let input = mastery::Snapshot {
            mastery: num(snapshot, "mastery"),
            strength: num(snapshot, "strength"),
            state: snapshot["state"].as_str().unwrap_or("new").to_string(),
            review_count: int(snapshot, "reviewCount"),
            lapse_count: int(snapshot, "lapseCount"),
            last_reviewed_at: snapshot["lastReviewedAt"].as_str().map(str::to_string),
        };

        let now = ms(case["now"].as_str().expect("now"));
        let actual = mastery::update_mastery(
            &input,
            num(case, "partialScore"),
            num(case, "difficulty"),
            now,
        );
        let expect = &case["expect"];
        let label = format!("mastery[{index}]");

        approx(actual.previous, num(expect, "previous"), &format!("{label}.previous"));
        approx(actual.next, num(expect, "next"), &format!("{label}.next"));
        approx(actual.delta, num(expect, "delta"), &format!("{label}.delta"));
        approx(actual.strength, num(expect, "strength"), &format!("{label}.strength"));
        assert_eq!(
            actual.state,
            expect["state"].as_str().expect("state"),
            "{label}.state"
        );
        assert_eq!(actual.review_count, int(expect, "reviewCount"), "{label}.reviewCount");
        assert_eq!(actual.lapse_count, int(expect, "lapseCount"), "{label}.lapseCount");
        assert_eq!(
            actual.lapsed,
            expect["lapsed"].as_bool().expect("lapsed"),
            "{label}.lapsed"
        );
    }
}

#[test]
fn scheduler_matches_typescript() {
    let data = fixture();
    let cases = data["srs"].as_array().expect("srs 数组");

    for (index, case) in cases.iter().enumerate() {
        let state = &case["state"];
        let rating = srs::Rating::parse(case["rating"].as_str().expect("rating")).expect("评分");
        let now = ms(case["now"].as_str().expect("now"));

        let actual = srs::schedule_next(
            srs::SchedulerState {
                ease: num(state, "ease"),
                interval_days: int(state, "intervalDays"),
                repetition: int(state, "repetition"),
                lapse_count: int(state, "lapseCount"),
            },
            rating,
            now,
        );

        let expect = &case["expect"];
        let label = format!("srs[{index}] {}", rating.as_str());

        approx(actual.ease, num(expect, "ease"), &format!("{label}.ease"));
        assert_eq!(
            actual.interval_days,
            int(expect, "intervalDays"),
            "{label}.intervalDays"
        );
        assert_eq!(actual.repetition, int(expect, "repetition"), "{label}.repetition");
        assert_eq!(
            actual.lapse_count,
            int(expect, "lapseCount"),
            "{label}.lapseCount"
        );
        assert_eq!(
            actual.due_at,
            expect["dueAt"].as_str().expect("dueAt"),
            "{label}.dueAt"
        );
    }
}

#[test]
fn xp_matches_typescript() {
    let data = fixture();

    for case in data["xp"].as_array().expect("xp 数组") {
        let total = int(case, "total");
        let actual = xp::level_from_xp(total);
        let expect = &case["expect"];
        let label = format!("xp[{total}]");

        assert_eq!(actual.level, int(expect, "level"), "{label}.level");
        assert_eq!(actual.current, int(expect, "current"), "{label}.current");
        assert_eq!(actual.required, int(expect, "required"), "{label}.required");
        approx(actual.progress, num(expect, "progress"), &format!("{label}.progress"));
    }

    for case in data["reviewXp"].as_array().expect("reviewXp 数组") {
        let rating = srs::Rating::parse(case["rating"].as_str().expect("rating")).expect("评分");
        assert_eq!(
            xp::xp_for_review(rating),
            int(case, "expect"),
            "复习经验不一致"
        );
    }
}

#[test]
fn selection_matches_typescript() {
    let data = fixture();
    let cases = data["selection"].as_array().expect("selection 数组");

    for (index, case) in cases.iter().enumerate() {
        let pool: Vec<Exercise> = case["pool"]
            .as_array()
            .expect("pool")
            .iter()
            .map(|item| Exercise {
                id: item["id"].as_str().expect("id").to_string(),
                kind: "spelling".to_string(),
                difficulty: int(item, "difficulty"),
                xp: 5,
                prompt: ExercisePrompt {
                    instruction: String::new(),
                    context: None,
                },
                entity_ids: Vec::new(),
                explanation: String::new(),
                tags: Vec::new(),
                payload: Json::Object(Map::new()),
            })
            .collect();

        let seen_at: HashMap<String, i64> = case["seenAt"]
            .as_object()
            .map(|map| {
                map.iter()
                    .map(|(key, value)| (key.clone(), value.as_i64().unwrap_or(0)))
                    .collect()
            })
            .unwrap_or_default();

        let picked = selection::select_exercises(
            &pool,
            int(case, "count") as usize,
            num(case, "targetDifficulty"),
            &seen_at,
            ms(case["now"].as_str().expect("now")),
        );

        let actual: Vec<&str> = picked.iter().map(|exercise| exercise.id.as_str()).collect();
        let expected: Vec<&str> = case["expectIds"]
            .as_array()
            .expect("expectIds")
            .iter()
            .map(|id| id.as_str().expect("id"))
            .collect();

        assert_eq!(actual, expected, "selection[{index}] 取题顺序不一致");
    }
}
