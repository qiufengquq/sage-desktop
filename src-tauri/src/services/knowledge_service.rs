//! 知识库读服务：把"内容"与"学习态"合成为前端要的 DTO。
//!
//! 前端在 mock 模式下用 `mergeWord/mergeGrammar/mergeSentence`（`knowledge.service.ts`）
//! 做同一件事；backend 模式下这份合成搬到这里，页面代码一行不用改。

use std::collections::HashMap;

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::algorithms::layout;
use crate::database::now_iso;
use crate::error::{AppError, AppResult};
use crate::models::knowledge::{
    to_json, EntityContent, GrammarPoint, IndexRow, KnowledgeFilter, KnowledgeGraph,
    KnowledgeNode, KnowledgeSummary, Sentence, Vocabulary,
};
use crate::models::state::EntityState;
use crate::repositories::{entity_repository, state_repository};

fn states(conn: &Connection) -> AppResult<HashMap<String, EntityState>> {
    Ok(state_repository::load_all(conn)?
        .into_iter()
        .map(|state| (state.entity_id.clone(), state))
        .collect())
}

/* --------------------------------- 列表 ---------------------------------- */

pub fn list(conn: &Connection, filter: &KnowledgeFilter) -> AppResult<Vec<KnowledgeSummary>> {
    let states = states(conn)?;
    let mut result = Vec::new();

    for row in entity_repository::index_rows(conn)? {
        if let Some(kind) = &filter.kind {
            if &row.kind != kind {
                continue;
            }
        }
        if let Some(difficulty) = filter.difficulty {
            if row.difficulty != difficulty {
                continue;
            }
        }

        let state = states.get(&row.id);
        let current = state.map(|s| s.state.clone()).unwrap_or_else(|| "new".to_string());

        if let Some(mastery) = &filter.mastery {
            if &current != mastery {
                continue;
            }
        }

        let tags = row.tags();
        if !filter.keyword.is_empty() && !matches_keyword(&row, &tags, &filter.keyword) {
            continue;
        }

        result.push(KnowledgeSummary {
            id: row.id,
            kind: row.kind,
            primary: row.primary,
            secondary: row.secondary,
            difficulty: row.difficulty,
            mastery: state.map(|s| s.mastery).unwrap_or(0.0),
            state: current,
            tags,
            due_at: state.and_then(|s| s.due_at.clone()),
        });
    }

    Ok(result)
}

fn matches_keyword(row: &IndexRow, tags: &[String], keyword: &str) -> bool {
    row.primary.to_lowercase().contains(keyword)
        || row.secondary.to_lowercase().contains(keyword)
        || tags.iter().any(|tag| tag.to_lowercase().contains(keyword))
}

/* --------------------------------- 图谱 ---------------------------------- */

pub fn graph(conn: &Connection) -> AppResult<KnowledgeGraph> {
    let states = states(conn)?;
    let rows = entity_repository::node_rows(conn)?;

    let is_vocabulary = |kind: &str| kind == "word" || kind == "phrase";
    let vocabulary_total = rows.iter().filter(|row| is_vocabulary(&row.kind)).count();
    let grammar_total = rows.iter().filter(|row| row.kind == "grammar").count();
    let sentence_total = rows.iter().filter(|row| row.kind == "sentence").count();

    let mut vocabulary_index = 0usize;
    let mut grammar_index = 0usize;
    let mut sentence_index = 0usize;

    let mut nodes = Vec::with_capacity(rows.len());
    for row in rows {
        let (index, total) = if is_vocabulary(&row.kind) {
            let index = vocabulary_index;
            vocabulary_index += 1;
            (index, vocabulary_total)
        } else if row.kind == "grammar" {
            let index = grammar_index;
            grammar_index += 1;
            (index, grammar_total)
        } else {
            let index = sentence_index;
            sentence_index += 1;
            (index, sentence_total)
        };

        let (x, y) = layout::seed_position(&row.kind, index, total);
        let state = states.get(&row.id);
        let mastery = state.map(|s| s.mastery).unwrap_or(0.0);

        nodes.push(KnowledgeNode {
            id: row.id.clone(),
            kind: row.kind.clone(),
            label: layout::node_label(&row.kind, &row.label, 26),
            sublabel: row.sublabel,
            difficulty: row.difficulty,
            mastery,
            state: state.map(|s| s.state.clone()).unwrap_or_else(|| "new".to_string()),
            x,
            y,
            weight: layout::node_weight(&row.kind, mastery),
        });
    }

    Ok(KnowledgeGraph {
        nodes,
        edges: entity_repository::edges(conn)?,
        updated_at: now_iso(),
    })
}

/* --------------------------------- 详情 ---------------------------------- */

pub fn detail(conn: &Connection, id: &str) -> AppResult<Value> {
    let content = entity_repository::entity_content(conn, id)?
        .ok_or_else(|| AppError::not_found(id))?;
    let states = states(conn)?;

    match content {
        EntityContent::Word(mut word) => {
            let entity_id = word.id.clone();
            apply_word_state(&mut word, states.get(&entity_id));
            let related_words = summaries(conn, &states, &word.related_word_ids)?;
            let grammar_points = grammars(conn, &states, &word.grammar_ids)?;
            let sentence_ids = entity_repository::sentences_referencing(conn, &entity_id)?;
            let sentence_values = sentences(conn, &states, &sentence_ids)?;

            let kind = if word.pos == "phrase" { "phrase" } else { "word" };
            Ok(json!({
                "kind": kind,
                "word": to_json(&word),
                "relatedWords": related_words,
                "grammarPoints": grammar_points,
                "sentences": sentence_values,
            }))
        }
        EntityContent::Grammar(mut grammar) => {
            let entity_id = grammar.id.clone();
            apply_grammar_state(&mut grammar, states.get(&entity_id));
            let words = summaries(conn, &states, &grammar.related_word_ids)?;
            let sentence_ids = entity_repository::sentences_referencing(conn, &entity_id)?;
            let sentence_values = sentences(conn, &states, &sentence_ids)?;

            Ok(json!({
                "kind": "grammar",
                "grammar": to_json(&grammar),
                "words": words,
                "sentences": sentence_values,
            }))
        }
        EntityContent::Sentence(mut sentence) => {
            let entity_id = sentence.id.clone();
            apply_sentence_state(&mut sentence, states.get(&entity_id));
            let grammar_points = grammars(conn, &states, &sentence.grammar_ids)?;
            let words = summaries(conn, &states, &sentence.word_ids)?;

            Ok(json!({
                "kind": "sentence",
                "sentence": to_json(&sentence),
                "grammarPoints": grammar_points,
                "words": words,
            }))
        }
    }
}

pub fn neighbors(conn: &Connection, id: &str) -> AppResult<Vec<KnowledgeSummary>> {
    let states = states(conn)?;
    let ids = entity_repository::neighbors_of(conn, id)?;
    summaries(conn, &states, &ids)
}

/* ------------------------------- 合成辅助 -------------------------------- */

fn summaries(
    conn: &Connection,
    states: &HashMap<String, EntityState>,
    ids: &[String],
) -> AppResult<Vec<KnowledgeSummary>> {
    let mut result = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(summary) = summary_of(conn, states, id)? {
            result.push(summary);
        }
    }
    Ok(result)
}

fn summary_of(
    conn: &Connection,
    states: &HashMap<String, EntityState>,
    id: &str,
) -> AppResult<Option<KnowledgeSummary>> {
    let Some(content) = entity_repository::entity_content(conn, id)? else {
        return Ok(None);
    };
    let state = states.get(id);
    let mastery = state.map(|s| s.mastery).unwrap_or(0.0);
    let current = state.map(|s| s.state.clone()).unwrap_or_else(|| "new".to_string());
    let due_at = state.and_then(|s| s.due_at.clone());

    let summary = match content {
        EntityContent::Word(mut word) => {
            apply_word_state(&mut word, state);
            KnowledgeSummary {
                id: word.id.clone(),
                kind: if word.pos == "phrase" { "phrase".into() } else { "word".into() },
                primary: word.term.clone(),
                secondary: word.meanings.first().map(|m| m.definition.clone()).unwrap_or_default(),
                difficulty: word.difficulty,
                mastery,
                state: current,
                tags: word.contexts.clone(),
                due_at,
            }
        }
        EntityContent::Grammar(grammar) => KnowledgeSummary {
            id: grammar.id.clone(),
            kind: "grammar".into(),
            primary: grammar.title.clone(),
            secondary: grammar.title_en.clone(),
            difficulty: grammar.difficulty,
            mastery,
            state: current,
            tags: vec![grammar.category.clone()],
            due_at,
        },
        EntityContent::Sentence(sentence) => KnowledgeSummary {
            id: sentence.id.clone(),
            kind: "sentence".into(),
            primary: sentence.en.clone(),
            secondary: sentence.zh.clone(),
            difficulty: sentence.difficulty,
            mastery,
            state: current,
            tags: vec![sentence.category.clone()],
            due_at,
        },
    };

    Ok(Some(summary))
}

fn grammars(
    conn: &Connection,
    states: &HashMap<String, EntityState>,
    ids: &[String],
) -> AppResult<Vec<Value>> {
    let mut result = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(mut grammar) = entity_repository::grammar(conn, id)? {
            apply_grammar_state(&mut grammar, states.get(id));
            result.push(to_json(&grammar));
        }
    }
    Ok(result)
}

fn sentences(
    conn: &Connection,
    states: &HashMap<String, EntityState>,
    ids: &[String],
) -> AppResult<Vec<Value>> {
    let mut result = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(mut sentence) = entity_repository::sentence(conn, id)? {
            apply_sentence_state(&mut sentence, states.get(id));
            result.push(to_json(&sentence));
        }
    }
    Ok(result)
}

fn apply_word_state(word: &mut Vocabulary, state: Option<&EntityState>) {
    let Some(state) = state else { return };
    word.mastery = state.mastery;
    word.strength = state.strength;
    word.state = state.state.clone();
    word.review_count = state.review_count;
    word.due_at = state.due_at.clone();
    word.last_reviewed_at = state.last_reviewed_at.clone();
}

fn apply_grammar_state(grammar: &mut GrammarPoint, state: Option<&EntityState>) {
    let Some(state) = state else { return };
    grammar.mastery = state.mastery;
    grammar.state = state.state.clone();
    grammar.review_count = state.review_count;
    grammar.due_at = state.due_at.clone();
    grammar.last_reviewed_at = state.last_reviewed_at.clone();
}

fn apply_sentence_state(sentence: &mut Sentence, state: Option<&EntityState>) {
    let Some(state) = state else { return };
    sentence.mastery = state.mastery;
    sentence.state = state.state.clone();
    sentence.review_count = state.review_count;
    sentence.due_at = state.due_at.clone();
    sentence.last_reviewed_at = state.last_reviewed_at.clone();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::repositories::state_repository;

    /// 内存库 + 内容包 + 全量状态行：与生产启动后的状态一致，且不争抢文件锁
    fn seeded_db() -> Database {
        let db = Database::open_in_memory_with_content().expect("open");
        db.with_tx(|tx| {
            state_repository::ensure_rows_for_all_entities(tx)?;
            Ok(())
        })
        .expect("seed states");
        db
    }

    #[test]
    fn list_returns_all_entities_in_content_order() {
        let db = seeded_db();
        let items = db
            .with_conn(|conn| list(conn, &KnowledgeFilter::default()))
            .expect("list");

        assert_eq!(items.len(), 36, "36 个知识实体");
        assert_eq!(items[0].kind, "word", "词在最前");
        assert!(items.iter().any(|item| item.kind == "grammar"));
        assert!(items.iter().any(|item| item.kind == "sentence"));
        assert!(items.iter().any(|item| item.kind == "phrase"));
    }

    #[test]
    fn list_filters_compose() {
        let db = seeded_db();
        db.with_conn(|conn| {
            let grammar_only = list(
                conn,
                &KnowledgeFilter {
                    kind: Some("grammar".into()),
                    ..Default::default()
                },
            )?;
            assert_eq!(grammar_only.len(), 10);

            let keyword = list(
                conn,
                &KnowledgeFilter {
                    keyword: "从句".into(),
                    ..Default::default()
                },
            )?;
            assert!(!keyword.is_empty(), "关键词应命中语法标题或释义");

            let level3 = list(
                conn,
                &KnowledgeFilter {
                    difficulty: Some(3),
                    ..Default::default()
                },
            )?;
            assert!(level3.iter().all(|item| item.difficulty == 3));
            Ok(())
        })
        .expect("filters");
    }

    #[test]
    fn graph_matches_content_scale() {
        let db = seeded_db();
        db.with_conn(|conn| graph(conn)).map(|graph| {
            assert_eq!(graph.nodes.len(), 36);
            assert_eq!(graph.edges.len(), 59, "与内容包的关系边数一致");
            for node in &graph.nodes {
                assert!((0.05..=0.95).contains(&node.x));
                assert!((0.08..=0.92).contains(&node.y));
                assert!(node.weight >= 1.0);
            }
        })
        .expect("graph");
    }

    #[test]
    fn detail_is_a_tagged_union() {
        let db = seeded_db();
        db.with_conn(|conn| {
            let word = list(
                conn,
                &KnowledgeFilter {
                    kind: Some("word".into()),
                    ..Default::default()
                },
            )?
            .first()
            .map(|item| item.id.clone())
            .expect("至少一个词");

            let word_detail = detail(conn, &word)?;
            assert_eq!(word_detail["kind"], "word");
            assert!(word_detail["word"]["term"].is_string());
            assert!(word_detail["relatedWords"].is_array());
            assert!(word_detail["grammarPoints"].is_array());
            assert!(word_detail["sentences"].is_array());

            let grammar_id = list(
                conn,
                &KnowledgeFilter {
                    kind: Some("grammar".into()),
                    ..Default::default()
                },
            )?[0]
                .id
                .clone();
            let grammar_detail = detail(conn, &grammar_id)?;
            assert_eq!(grammar_detail["kind"], "grammar");
            assert!(grammar_detail["words"].is_array());

            let sentence_id = list(
                conn,
                &KnowledgeFilter {
                    kind: Some("sentence".into()),
                    ..Default::default()
                },
            )?[0]
                .id
                .clone();
            let sentence_detail = detail(conn, &sentence_id)?;
            assert_eq!(sentence_detail["kind"], "sentence");
            assert!(sentence_detail["grammarPoints"].is_array());
            Ok(())
        })
        .expect("detail");
    }

    #[test]
    fn detail_of_unknown_id_is_not_found() {
        let db = seeded_db();
        let outcome = db.with_conn(|conn| detail(conn, "w_missing"));
        match outcome {
            Err(err) => assert!(err.to_string().starts_with("not_found:")),
            Ok(_) => panic!("不存在的实体应返回 not_found"),
        }
    }

    #[test]
    fn neighbors_are_directly_connected() {
        let db = seeded_db();
        db.with_conn(|conn| {
            let word_id = list(
                conn,
                &KnowledgeFilter {
                    kind: Some("word".into()),
                    ..Default::default()
                },
            )?[0]
                .id
                .clone();

            let neighbors = neighbors(conn, &word_id)?;
            assert!(!neighbors.is_empty(), "第一个词应当有关联");
            assert!(neighbors.iter().all(|item| item.id != word_id));
            Ok(())
        })
        .expect("neighbors");
    }

    #[test]
    fn state_overlay_reaches_summary() {
        let db = seeded_db();
        db.with_tx(|tx| {
            let id: String = tx.query_row(
                "SELECT entity_id FROM entity_states WHERE kind = 'word' ORDER BY entity_id LIMIT 1",
                [],
                |row| row.get(0),
            )?;
            tx.execute(
                "UPDATE entity_states SET mastery = 77.5, state = 'reviewing',
                   review_count = 4, due_at = '2026-11-20T00:00:00.000Z'
                 WHERE entity_id = ?1",
                [&id],
            )?;
            Ok(())
        })
        .expect("set state");

        let items = db
            .with_conn(|conn| list(conn, &KnowledgeFilter::default()))
            .expect("list");
        let target = items
            .iter()
            .find(|item| item.state == "reviewing")
            .expect("应读到刚写入的状态");
        assert_eq!(target.mastery, 77.5);
        assert_eq!(target.due_at.as_deref(), Some("2026-11-20T00:00:00.000Z"));
    }
}
