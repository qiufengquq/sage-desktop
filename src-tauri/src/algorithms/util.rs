//! 与 `src/lib/utils.ts` 对齐的小工具。
//!
//! 归一化规则必须逐字符一致：判分与复习调度都建立在它之上，
//! 两端任何差异都会让"同一份答案"得到不同分数。

pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.min(max).max(min)
}

/// 归一化用户输入：去首尾空白、转小写、去标点、压缩空格
pub fn normalize_answer(input: &str) -> String {
    let lowered = input.trim().to_lowercase();
    let stripped: String = lowered
        .chars()
        .filter(|ch| !matches!(ch, '.' | ',' | '!' | '?' | ';' | ':' | '"' | '\'' | '’'))
        .collect();
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 词级重叠度：交集词数 / 较长者词数
pub fn similarity(a: &str, b: &str) -> f64 {
    let left = normalize_answer(a);
    let right = normalize_answer(b);

    let wa: Vec<&str> = left.split(' ').filter(|word| !word.is_empty()).collect();
    let wb: Vec<&str> = right.split(' ').filter(|word| !word.is_empty()).collect();

    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }

    let set_b: std::collections::HashSet<&str> = wb.iter().copied().collect();
    let hits = wa.iter().filter(|word| set_b.contains(*word)).count();
    hits as f64 / wa.len().max(wb.len()) as f64
}

/// 字符级相似度（双字符组 Dice 系数）：拼写/听写给部分分
pub fn character_similarity(a: &str, b: &str) -> f64 {
    let x: String = normalize_answer(a).split_whitespace().collect();
    let y: String = normalize_answer(b).split_whitespace().collect();

    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    if x == y {
        return 1.0;
    }

    let bigrams = |value: &str| -> std::collections::HashMap<(char, char), i64> {
        let chars: Vec<char> = value.chars().collect();
        let mut counts = std::collections::HashMap::new();
        if chars.len() < 2 {
            return counts;
        }
        for pair in chars.windows(2) {
            *counts.entry((pair[0], pair[1])).or_insert(0) += 1;
        }
        counts
    };

    let x_grams = bigrams(&x);
    let y_grams = bigrams(&y);
    if x_grams.is_empty() || y_grams.is_empty() {
        return 0.0;
    }

    let mut intersection = 0i64;
    for (gram, count) in &x_grams {
        let other = y_grams.get(gram).copied().unwrap_or(0);
        intersection += (*count).min(other);
    }

    // 注意：分母用的是字符数而非 gram 数，与 TS 实现保持一致
    let total = (x.chars().count() as f64 - 1.0) + (y.chars().count() as f64 - 1.0);
    if total == 0.0 {
        return 0.0;
    }
    (2.0 * intersection as f64) / total
}

/// 保留一位小数（与 JS `Math.round(v * 10) / 10` 一致：正数时等价）
pub fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// 保留三位小数
pub fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

pub fn round0(value: f64) -> f64 {
    value.round()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_matches_frontend_rules() {
        assert_eq!(normalize_answer("  Hello,   World!  "), "hello world");
        assert_eq!(normalize_answer("It's  fine."), "its fine");
        assert_eq!(normalize_answer("a\tb\nc"), "a b c");
    }

    #[test]
    fn similarity_is_word_overlap() {
        assert_eq!(similarity("I like tea", "I like tea"), 1.0);
        assert_eq!(similarity("I like tea", "I like coffee"), 2.0 / 3.0);
        assert_eq!(similarity("", "abc"), 0.0);
    }

    #[test]
    fn character_similarity_gives_partial_credit() {
        assert_eq!(character_similarity("apple", "apple"), 1.0);
        let near = character_similarity("appl", "apple");
        assert!(near > 0.5 && near < 1.0, "少一个字母应拿到部分分：{near}");
        assert_eq!(character_similarity("", "apple"), 0.0);
    }

    #[test]
    fn rounding_matches_js() {
        assert_eq!(round1(2.25), 2.3);
        assert_eq!(round1(2.24), 2.2);
        assert_eq!(round3(0.12345), 0.123);
    }
}
