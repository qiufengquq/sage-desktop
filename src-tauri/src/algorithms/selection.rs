//! 取题策略（对照组：`src/lib/algorithms/selection.ts`）。
//!
//! 评分（越小越优先）：
//!   难度差 × 2 + 近期惩罚（1 天内 +6，3 天内 +3，7 天内 +1）
//! 同分按题库原始顺序，保证"同样的输入得到同样的题目"。

use std::collections::HashMap;

use crate::models::exercise::Exercise;

const DAY_MS: i64 = 86_400_000;

pub fn select_exercises(
    pool: &[Exercise],
    count: usize,
    target_difficulty: f64,
    seen_at: &HashMap<String, i64>,
    now_ms: i64,
) -> Vec<Exercise> {
    let mut scored: Vec<(usize, f64)> = pool
        .iter()
        .enumerate()
        .map(|(index, exercise)| {
            let mut recent_penalty = 0.0;
            if let Some(last_seen) = seen_at.get(&exercise.id) {
                let days = (now_ms - last_seen) as f64 / DAY_MS as f64;
                if days < 1.0 {
                    recent_penalty = 6.0;
                } else if days < 3.0 {
                    recent_penalty = 3.0;
                } else if days < 7.0 {
                    recent_penalty = 1.0;
                }
            }
            let gap = (exercise.difficulty as f64 - target_difficulty).abs();
            (index, gap * 2.0 + recent_penalty)
        })
        .collect();

    scored.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });

    scored
        .into_iter()
        .take(count)
        .map(|(index, _)| pool[index].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn exercise(id: &str, difficulty: i64) -> Exercise {
        Exercise {
            id: id.to_string(),
            kind: "spelling".to_string(),
            difficulty,
            xp: 10,
            prompt: crate::models::exercise::ExercisePrompt {
                instruction: "拼写".to_string(),
                context: None,
            },
            entity_ids: vec!["w_demo".to_string()],
            explanation: String::new(),
            tags: vec![],
            payload: json!({ "answer": "demo" }),
        }
    }

    #[test]
    fn prefers_difficulty_close_to_target() {
        let pool = vec![exercise("a", 5), exercise("b", 3), exercise("c", 1)];
        let picked = select_exercises(&pool, 1, 3.0, &HashMap::new(), 0);
        assert_eq!(picked[0].id, "b");
    }

    #[test]
    fn recently_seen_items_are_deprioritised() {
        let pool = vec![exercise("a", 3), exercise("b", 3)];
        let mut seen = HashMap::new();
        seen.insert("a".to_string(), 1_000_000);
        let picked = select_exercises(&pool, 1, 3.0, &seen, 1_000_000);
        assert_eq!(picked[0].id, "b", "刚做过的题应排到后面");
    }

    #[test]
    fn count_is_respected_and_order_is_deterministic() {
        let pool = vec![exercise("a", 3), exercise("b", 3), exercise("c", 3)];
        let first = select_exercises(&pool, 2, 3.0, &HashMap::new(), 0);
        let second = select_exercises(&pool, 2, 3.0, &HashMap::new(), 0);
        assert_eq!(first.len(), 2);
        assert_eq!(
            first.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
            second.iter().map(|e| e.id.clone()).collect::<Vec<_>>()
        );
        assert_eq!(first[0].id, "a", "同分保持题库顺序");
    }
}
