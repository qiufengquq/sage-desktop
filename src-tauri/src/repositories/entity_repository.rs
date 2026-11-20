//! 知识实体仓储：列表、详情、关系、图谱所需的内容读取。
//!
//! 只返回"内容"，不掺学习态；掌握度由 service 层用 `entity_states` 叠加，
//! 这样内容与状态永远只有一个写入点。

use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::AppResult;
use crate::models::knowledge::{
    json_array, EntityContent, GrammarExample, GrammarMistake, GrammarPoint, IndexRow,
    KnowledgeEdge, NodeRow, Sentence, Vocabulary, WordExample, WordMeaning,
};

/* ---------------------------------- 列表 ---------------------------------- */

pub fn index_rows(conn: &Connection) -> AppResult<Vec<IndexRow>> {
    // 按内容插入顺序返回（词 → 语法 → 句子），与第一阶段列表顺序一致
    let mut stmt = conn.prepare(
        "SELECT ki.id, ki.kind, ki.difficulty, ki.primary_text, ki.secondary_text,
                COALESCE(v.contexts_json, '[]') AS contexts_json,
                g.category AS grammar_category,
                s.category AS sentence_category
           FROM knowledge_index ki
           JOIN entities          e ON e.id = ki.id
           LEFT JOIN vocabulary     v ON v.entity_id = ki.id
           LEFT JOIN grammar_points g ON g.entity_id = ki.id
           LEFT JOIN sentences      s ON s.entity_id = ki.id
          ORDER BY e.rowid",
    )?;
    let rows = stmt.query_map([], IndexRow::from_row)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/* ---------------------------------- 图谱 ---------------------------------- */

/// 按内容插入顺序返回（rowid 顺序），布局坐标依赖这个顺序
pub fn node_rows(conn: &Connection) -> AppResult<Vec<NodeRow>> {
    let mut stmt = conn.prepare(
        "SELECT e.id, e.kind, e.difficulty, ki.primary_text AS label, ki.secondary_text AS sublabel
           FROM entities e
           JOIN knowledge_index ki ON ki.id = e.id
          ORDER BY e.rowid",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(NodeRow {
            id: row.get("id")?,
            kind: row.get("kind")?,
            difficulty: row.get("difficulty")?,
            label: row.get("label")?,
            sublabel: row.get("sublabel")?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn edges(conn: &Connection) -> AppResult<Vec<KnowledgeEdge>> {
    let mut stmt = conn.prepare(
        "SELECT source_id, target_id, relation, weight FROM knowledge_relations ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        let source: String = row.get(0)?;
        let target: String = row.get(1)?;
        Ok(KnowledgeEdge {
            id: format!("e_{source}_{target}"),
            source,
            target,
            relation: row.get(2)?,
            weight: row.get(3)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// 邻接关系（无向）：按边插入顺序去重，与前端 `neighborsOf` 的顺序一致
pub fn neighbors_of(conn: &Connection, entity_id: &str) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT CASE WHEN source_id = ?1 THEN target_id ELSE source_id END
           FROM knowledge_relations
          WHERE source_id = ?1 OR target_id = ?1
          ORDER BY id",
    )?;
    let rows = stmt.query_map([entity_id], |row| row.get::<_, String>(0))?;

    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for row in rows {
        let id = row?;
        if seen.insert(id.clone()) {
            result.push(id);
        }
    }
    Ok(result)
}

/// 某个实体在指定关系下的目标 id
pub fn related_ids(conn: &Connection, entity_id: &str, relation: &str) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT target_id FROM knowledge_relations
          WHERE source_id = ?1 AND relation = ?2 ORDER BY id",
    )?;
    let rows = stmt.query_map(rusqlite::params![entity_id, relation], |row| row.get::<_, String>(0))?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// 句子关联的语法 id（关系表里用 used-in 连接，需要按目标类型区分）
pub fn related_ids_of_kind(
    conn: &Connection,
    entity_id: &str,
    relation: &str,
    kinds: &[&str],
) -> AppResult<Vec<String>> {
    let placeholders = kinds
        .iter()
        .enumerate()
        .map(|(index, _)| format!("?{}", index + 2))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT r.target_id FROM knowledge_relations r
           JOIN entities e ON e.id = r.target_id
          WHERE r.source_id = ?1 AND r.relation = '{relation}' AND e.kind IN ({placeholders})
          ORDER BY r.id"
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&entity_id];
    for kind in kinds {
        params.push(kind);
    }
    let rows = stmt.query_map(params.as_slice(), |row| row.get::<_, String>(0))?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/* ---------------------------------- 详情 ---------------------------------- */

pub fn entity_content(conn: &Connection, id: &str) -> AppResult<Option<EntityContent>> {
    let kind: Option<String> = conn
        .query_row("SELECT kind FROM entities WHERE id = ?1", [id], |row| row.get(0))
        .ok();

    match kind.as_deref() {
        Some("word") | Some("phrase") => Ok(vocabulary(conn, id)?.map(EntityContent::Word)),
        Some("grammar") => Ok(grammar(conn, id)?.map(EntityContent::Grammar)),
        Some("sentence") => Ok(sentence(conn, id)?.map(EntityContent::Sentence)),
        _ => Ok(None),
    }
}

pub fn vocabulary(conn: &Connection, id: &str) -> AppResult<Option<Vocabulary>> {
    let head = conn
        .query_row(
            "SELECT v.entity_id, v.term, v.phonetic, v.pos, v.contexts_json,
                    e.difficulty, e.level, e.created_at
               FROM vocabulary v
               JOIN entities e ON e.id = v.entity_id
              WHERE v.entity_id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .ok();

    let Some((entity_id, term, phonetic, pos, contexts, difficulty, level, added_at)) = head else {
        return Ok(None);
    };

    let mut meaning_stmt = conn.prepare(
        "SELECT pos, definition, definition_en FROM word_meanings
          WHERE entity_id = ?1 ORDER BY sort_order",
    )?;
    let meanings = meaning_stmt
        .query_map([id], |row| {
            Ok(WordMeaning {
                pos: row.get(0)?,
                definition: row.get(1)?,
                definition_en: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut example_stmt = conn.prepare(
        "SELECT id, en, zh, highlight FROM word_examples
          WHERE entity_id = ?1 ORDER BY sort_order",
    )?;
    let examples = example_stmt
        .query_map([id], |row| {
            Ok(WordExample {
                id: row.get(0)?,
                en: row.get(1)?,
                zh: row.get(2)?,
                highlight: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(Some(Vocabulary {
        id: entity_id,
        term,
        phonetic,
        pos,
        difficulty,
        level,
        meanings,
        examples,
        related_word_ids: related_ids(conn, id, "related")?,
        grammar_ids: related_ids(conn, id, "explains")?,
        contexts: json_array(&contexts),
        mastery: 0.0,
        state: "new".to_string(),
        review_count: 0,
        due_at: None,
        strength: 0.0,
        added_at,
        last_reviewed_at: None,
    }))
}

pub fn grammar(conn: &Connection, id: &str) -> AppResult<Option<GrammarPoint>> {
    let head = conn
        .query_row(
            "SELECT g.entity_id, g.title, g.title_en, g.category, g.summary, g.structure,
                    g.explanation, e.difficulty, e.level
               FROM grammar_points g
               JOIN entities e ON e.id = g.entity_id
              WHERE g.entity_id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .ok();

    let Some((entity_id, title, title_en, category, summary, structure, explanation, difficulty, level)) =
        head
    else {
        return Ok(None);
    };

    let mut example_stmt = conn.prepare(
        "SELECT id, en, zh, note FROM grammar_examples WHERE entity_id = ?1 ORDER BY sort_order",
    )?;
    let examples = example_stmt
        .query_map([id], |row| {
            Ok(GrammarExample {
                id: row.get(0)?,
                en: row.get(1)?,
                zh: row.get(2)?,
                note: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut mistake_stmt = conn.prepare(
        "SELECT id, wrong, correct, reason FROM grammar_mistakes
          WHERE entity_id = ?1 ORDER BY sort_order",
    )?;
    let mistakes = mistake_stmt
        .query_map([id], |row| {
            Ok(GrammarMistake {
                id: row.get(0)?,
                wrong: row.get(1)?,
                correct: row.get(2)?,
                reason: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(Some(GrammarPoint {
        id: entity_id,
        title,
        title_en,
        category,
        difficulty,
        level,
        summary,
        structure,
        explanation,
        examples,
        mistakes,
        related_word_ids: related_ids_of_kind(conn, id, "used-in", &["word", "phrase"])?,
        mastery: 0.0,
        state: "new".to_string(),
        review_count: 0,
        due_at: None,
        last_reviewed_at: None,
    }))
}

pub fn sentence(conn: &Connection, id: &str) -> AppResult<Option<Sentence>> {
    let head = conn
        .query_row(
            "SELECT s.entity_id, s.en, s.zh, s.category, s.chunks_json, e.difficulty, e.level
               FROM sentences s
               JOIN entities e ON e.id = s.entity_id
              WHERE s.entity_id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .ok();

    let Some((entity_id, en, zh, category, chunks, difficulty, level)) = head else {
        return Ok(None);
    };

    Ok(Some(Sentence {
        id: entity_id,
        en,
        zh,
        category,
        difficulty,
        level,
        grammar_ids: related_ids_of_kind(conn, id, "used-in", &["grammar"])?,
        word_ids: related_ids_of_kind(conn, id, "used-in", &["word", "phrase"])?,
        chunks: json_array(&chunks),
        mastery: 0.0,
        state: "new".to_string(),
        review_count: 0,
        due_at: None,
        last_reviewed_at: None,
    }))
}

/// 引用了某个实体的句子（句子 → 实体走 `used-in` 边），用于详情页反向关联
pub fn sentences_referencing(conn: &Connection, entity_id: &str) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT r.source_id FROM knowledge_relations r
           JOIN entities e ON e.id = r.source_id
          WHERE r.target_id = ?1 AND r.relation = 'used-in' AND e.kind = 'sentence'
          ORDER BY r.id",
    )?;
    let rows = stmt.query_map([entity_id], |row| row.get::<_, String>(0))?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// 一次性取出所有实体的 id → kind，用于关系与邻居的批量解析
pub fn entity_kinds(conn: &Connection) -> AppResult<HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT id, kind FROM entities")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut map = HashMap::new();
    for row in rows {
        let (id, kind) = row?;
        map.insert(id, kind);
    }
    Ok(map)
}
