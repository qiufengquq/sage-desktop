//! 知识库的读模型。对应 `src/types/{vocabulary,grammar,sentence,knowledge}.ts`。
//!
//! 这里的结构是"内容 + 学习态"的合成结果：内容来自内容表，掌握度来自 `entity_states`。
//! 前端拿到的形状与第一阶段完全一致，因此页面不需要任何改动。

use rusqlite::Row;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

/* --------------------------------- 内容 ---------------------------------- */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordMeaning {
    pub pos: String,
    pub definition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition_en: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordExample {
    pub id: String,
    pub en: String,
    pub zh: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Vocabulary {
    pub id: String,
    pub term: String,
    pub phonetic: String,
    pub pos: String,
    pub difficulty: i64,
    pub level: String,
    pub meanings: Vec<WordMeaning>,
    pub examples: Vec<WordExample>,
    pub related_word_ids: Vec<String>,
    pub grammar_ids: Vec<String>,
    pub contexts: Vec<String>,
    pub mastery: f64,
    pub state: String,
    pub review_count: i64,
    pub due_at: Option<String>,
    pub strength: f64,
    pub added_at: String,
    pub last_reviewed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrammarExample {
    pub id: String,
    pub en: String,
    pub zh: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrammarMistake {
    pub id: String,
    pub wrong: String,
    pub correct: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrammarPoint {
    pub id: String,
    pub title: String,
    pub title_en: String,
    pub category: String,
    pub difficulty: i64,
    pub level: String,
    pub summary: String,
    pub structure: String,
    pub explanation: String,
    pub examples: Vec<GrammarExample>,
    pub mistakes: Vec<GrammarMistake>,
    pub related_word_ids: Vec<String>,
    pub mastery: f64,
    pub state: String,
    pub review_count: i64,
    pub due_at: Option<String>,
    pub last_reviewed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sentence {
    pub id: String,
    pub en: String,
    pub zh: String,
    pub category: String,
    pub difficulty: i64,
    pub level: String,
    pub grammar_ids: Vec<String>,
    pub word_ids: Vec<String>,
    pub chunks: Vec<String>,
    pub mastery: f64,
    pub state: String,
    pub review_count: i64,
    pub due_at: Option<String>,
    pub last_reviewed_at: Option<String>,
}

/* -------------------------------- 合成视图 -------------------------------- */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSummary {
    pub id: String,
    pub kind: String,
    pub primary: String,
    pub secondary: String,
    pub difficulty: i64,
    pub mastery: f64,
    pub state: String,
    pub tags: Vec<String>,
    pub due_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub sublabel: String,
    pub difficulty: i64,
    pub mastery: f64,
    pub state: String,
    pub x: f64,
    pub y: f64,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub relation: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeGraph {
    pub nodes: Vec<KnowledgeNode>,
    pub edges: Vec<KnowledgeEdge>,
    pub updated_at: String,
}

/* --------------------------------- 过滤条件 -------------------------------- */

/// 前端的筛选条件是"联合类型 + 'all' 字面量"，这里统一解析成 Option。
#[derive(Debug, Clone, Default)]
pub struct KnowledgeFilter {
    pub keyword: String,
    pub kind: Option<String>,
    pub difficulty: Option<i64>,
    pub mastery: Option<String>,
}

impl KnowledgeFilter {
    pub fn from_json(raw: Option<&Json>) -> Self {
        let Some(Json::Object(map)) = raw else {
            return Self::default();
        };

        Self {
            keyword: map
                .get("keyword")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .trim()
                .to_lowercase(),
            kind: string_filter(map, "kind"),
            difficulty: map.get("difficulty").and_then(|value| match value {
                Json::Number(number) => number.as_i64(),
                _ => None,
            }),
            mastery: string_filter(map, "mastery"),
        }
    }
}

fn string_filter(map: &Map<String, Json>, key: &str) -> Option<String> {
    match map.get(key) {
        Some(Json::String(text)) if text != "all" => Some(text.clone()),
        _ => None,
    }
}

/* --------------------------------- 行映射 --------------------------------- */

/// `knowledge_index` 视图 + 子类型表拼出的列表行
#[derive(Debug, Clone)]
pub struct IndexRow {
    pub id: String,
    pub kind: String,
    pub difficulty: i64,
    pub primary: String,
    pub secondary: String,
    pub contexts: Vec<String>,
    pub grammar_category: Option<String>,
    pub sentence_category: Option<String>,
}

impl IndexRow {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let contexts_raw: String = row.get("contexts_json")?;
        Ok(Self {
            id: row.get("id")?,
            kind: row.get("kind")?,
            difficulty: row.get("difficulty")?,
            primary: row.get("primary_text")?,
            secondary: row.get("secondary_text")?,
            contexts: serde_json::from_str(&contexts_raw).unwrap_or_default(),
            grammar_category: row.get("grammar_category")?,
            sentence_category: row.get("sentence_category")?,
        })
    }

    /// 与前端 `wordSummary/grammarSummary/sentenceSummary` 的 tags 规则一致
    pub fn tags(&self) -> Vec<String> {
        if !self.contexts.is_empty() {
            return self.contexts.clone();
        }
        if let Some(category) = &self.grammar_category {
            return vec![category.clone()];
        }
        if let Some(category) = &self.sentence_category {
            return vec![category.clone()];
        }
        Vec::new()
    }
}

/// 图谱节点所需的轻量实体行
#[derive(Debug, Clone)]
pub struct NodeRow {
    pub id: String,
    pub kind: String,
    pub difficulty: i64,
    pub label: String,
    pub sublabel: String,
}

/* ------------------------------- 已解析内容 ------------------------------- */

/// 详情页需要的一份"实体内容"，与学习态无关
#[derive(Debug, Clone)]
pub enum EntityContent {
    Word(Vocabulary),
    Grammar(GrammarPoint),
    Sentence(Sentence),
}

impl EntityContent {
    pub fn id(&self) -> &str {
        match self {
            EntityContent::Word(word) => &word.id,
            EntityContent::Grammar(grammar) => &grammar.id,
            EntityContent::Sentence(sentence) => &sentence.id,
        }
    }

    pub fn kind(&self) -> &str {
        match self {
            EntityContent::Word(word) => {
                if word.pos == "phrase" {
                    "phrase"
                } else {
                    "word"
                }
            }
            EntityContent::Grammar(_) => "grammar",
            EntityContent::Sentence(_) => "sentence",
        }
    }
}

/// 序列化前的辅助：把可辨识联合拼成前端要的对象
pub fn tagged(pairs: Vec<(&str, Json)>) -> Json {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Json::Object(map)
}

/// 反序列化 `*_json` 列（失败时回退为空数组，避免一处脏数据拖垮整页）
pub fn json_array(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// 供测试与命令行工具使用：把任意 Serialize 值转成 JSON
pub fn to_json<T: Serialize>(value: &T) -> Json {
    serde_json::to_value(value).unwrap_or(Json::Null)
}

/// 反序列化辅助（内容包同步时用）
pub fn from_json<'a, T: Deserialize<'a>>(value: &'a Json) -> Option<T> {
    T::deserialize(value).ok()
}
