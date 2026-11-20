//! 知识图谱的确定性布局。
//!
//! 与 `src/services/mock/knowledge.ts` 的 `seedPosition` 完全一致：
//! 以语义簇为锚点，用黄金角螺旋撒点，保证同一份内容每次打开坐标相同
//! （不会"跳一下"）。坐标不落库，纯计算。

/// 各语义簇的锚点（归一化 0–1）
fn anchor_of(kind: &str) -> (f64, f64) {
    match kind {
        "phrase" => (0.5, 0.2),
        "grammar" => (0.7, 0.52),
        "sentence" => (0.42, 0.76),
        _ => (0.34, 0.42),
    }
}

/// 黄金角（弧度）
const GOLDEN_ANGLE: f64 = 2.399963;

pub fn seed_position(kind: &str, index: usize, total: usize) -> (f64, f64) {
    let (anchor_x, anchor_y) = anchor_of(kind);
    let radius = 0.1 + 0.055 * ((index + 1) as f64).sqrt();
    let angle = index as f64 * GOLDEN_ANGLE;
    let jitter = if total > 1 { 1.0 } else { 0.0 };

    let x = (anchor_x + angle.cos() * radius * jitter).clamp(0.05, 0.95);
    let y = (anchor_y + angle.sin() * radius * jitter * 0.9).clamp(0.08, 0.92);
    (x, y)
}

/// 节点权重：越大越"重"，决定半径
pub fn node_weight(kind: &str, mastery: f64) -> f64 {
    match kind {
        "grammar" => 1.4 + mastery / 120.0,
        "sentence" => 1.2 + mastery / 140.0,
        _ => 1.0 + mastery / 100.0,
    }
}

/// 句子在图上只显示前 26 个字符，避免节点文字过长
pub fn node_label(kind: &str, label: &str, max_chars: usize) -> String {
    if kind != "sentence" {
        return label.to_string();
    }
    let chars: Vec<char> = label.chars().collect();
    if chars.len() <= max_chars + 2 {
        return label.to_string();
    }
    let head: String = chars.into_iter().take(max_chars).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_are_stable_and_bounded() {
        let first = seed_position("word", 3, 18);
        let second = seed_position("word", 3, 18);
        assert_eq!(first, second, "同一输入必须得到同一坐标");
        assert!((0.05..=0.95).contains(&first.0));
        assert!((0.08..=0.92).contains(&first.1));
    }

    #[test]
    fn single_entity_sits_on_anchor() {
        let (x, y) = seed_position("grammar", 0, 1);
        assert!((x - 0.7).abs() < 1e-9, "只有一条时应收在锚点上");
        assert!((y - 0.52).abs() < 1e-9);
    }

    #[test]
    fn long_sentence_label_is_truncated() {
        let long = "This is a fairly long sentence that should be shortened for the graph view";
        let label = node_label("sentence", long, 26);
        assert!(label.ends_with('…'));
        assert_eq!(label.chars().count(), 27);
        assert_eq!(node_label("word", "brief", 26), "brief");
    }
}
