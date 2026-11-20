/**
 * 内容包导出：把前端 TS 内容模块（`src/services/mock/*`）导出为
 * `src-tauri/src/content/content_pack.json`，供 Rust 编译期内嵌并同步进 SQLite。
 *
 * 单一事实源是 TS；JSON 是生成物，不允许手改。
 *
 *   node scripts/export-content.mjs            # 生成
 *   node scripts/export-content.mjs --check    # 只校验（与已提交内容不一致则退出码 1）
 */
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { register } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..");
const OUT = resolve(ROOT, "src-tauri/src/content/content_pack.json");

// mock/base.ts 读取 import.meta.env，这里给它一个空对象
globalThis.__SAGE_ENV__ = {};
register("./alias-loader.mjs", pathToFileURL(`${HERE}/`));

const { allWords } = await import(pathToFileURL(resolve(ROOT, "src/services/mock/vocabulary.ts")).href);
const { allGrammar } = await import(pathToFileURL(resolve(ROOT, "src/services/mock/grammar.ts")).href);
const { allSentences } = await import(pathToFileURL(resolve(ROOT, "src/services/mock/sentences.ts")).href);
const exercisesModule = await import(pathToFileURL(resolve(ROOT, "src/services/mock/exercises.ts")).href);
const { assessmentModules } = await import(pathToFileURL(resolve(ROOT, "src/services/mock/assessment.ts")).href);

const EXERCISE_KINDS = [
  "word-match",
  "multiple-choice",
  "spelling",
  "dictation",
  "cloze",
  "translate",
  "sentence-order",
  "error-correction",
  "rewrite",
  "shadowing",
  "roleplay",
];

/** 题目上不属于 `payload_json` 的字段 */
const EXERCISE_BASE_KEYS = new Set(["id", "kind", "prompt", "difficulty", "entityIds", "explanation", "xp", "tags"]);

const json = (value) => JSON.stringify(value ?? []);
const orNull = (value) => (value === undefined ? null : value);

function buildPack() {
  const words = allWords();
  const grammar = allGrammar();
  const sentences = allSentences();

  const entities = [];
  const vocabulary = [];
  const word_meanings = [];
  const word_examples = [];
  const grammar_points = [];
  const grammar_examples = [];
  const grammar_mistakes = [];
  const sentenceRows = [];

  for (const word of words) {
    const kind = word.pos === "phrase" ? "phrase" : "word";
    entities.push({ id: word.id, kind, difficulty: word.difficulty, level: word.level });
    vocabulary.push({
      entity_id: word.id,
      term: word.term,
      phonetic: word.phonetic,
      pos: word.pos,
      contexts_json: json(word.contexts),
    });
    (word.meanings ?? []).forEach((meaning, index) => {
      word_meanings.push({
        entity_id: word.id,
        sort_order: index,
        pos: meaning.pos,
        definition: meaning.definition,
        definition_en: orNull(meaning.definitionEn),
      });
    });
    (word.examples ?? []).forEach((example, index) => {
      word_examples.push({
        id: example.id,
        entity_id: word.id,
        sort_order: index,
        en: example.en,
        zh: example.zh,
        highlight: orNull(example.highlight),
      });
    });
  }

  for (const point of grammar) {
    entities.push({ id: point.id, kind: "grammar", difficulty: point.difficulty, level: point.level });
    grammar_points.push({
      entity_id: point.id,
      title: point.title,
      title_en: point.titleEn,
      category: point.category,
      summary: point.summary,
      structure: point.structure,
      explanation: point.explanation,
    });
    (point.examples ?? []).forEach((example, index) => {
      grammar_examples.push({
        id: example.id,
        entity_id: point.id,
        sort_order: index,
        en: example.en,
        zh: example.zh,
        note: orNull(example.note),
      });
    });
    (point.mistakes ?? []).forEach((mistake, index) => {
      grammar_mistakes.push({
        id: mistake.id,
        entity_id: point.id,
        sort_order: index,
        wrong: mistake.wrong,
        correct: mistake.correct,
        reason: mistake.reason,
      });
    });
  }

  for (const sentence of sentences) {
    entities.push({ id: sentence.id, kind: "sentence", difficulty: sentence.difficulty, level: sentence.level });
    sentenceRows.push({
      entity_id: sentence.id,
      en: sentence.en,
      zh: sentence.zh,
      category: sentence.category,
      chunks_json: json(sentence.chunks),
    });
  }

  // 关系边：与 src/services/mock/knowledge.ts 的取边顺序、权重保持一致
  const entityIds = new Set(entities.map((item) => item.id));
  const knowledge_relations = [];
  const seenPairs = new Set();
  const pushRelation = (source, target, relation, weight) => {
    if (!entityIds.has(source) || !entityIds.has(target) || source === target) return;
    const key = [source, target].sort().join("|");
    if (seenPairs.has(key)) return;
    seenPairs.add(key);
    knowledge_relations.push({ source_id: source, target_id: target, relation, weight });
  };

  for (const word of words) {
    for (const grammarId of word.grammarIds ?? []) pushRelation(word.id, grammarId, "explains", 0.8);
    for (const relatedId of word.relatedWordIds ?? []) pushRelation(word.id, relatedId, "related", 1);
  }
  for (const point of grammar) {
    for (const wordId of point.relatedWordIds ?? []) pushRelation(point.id, wordId, "used-in", 0.7);
  }
  for (const sentence of sentences) {
    for (const grammarId of sentence.grammarIds ?? []) pushRelation(sentence.id, grammarId, "used-in", 1.1);
    for (const wordId of sentence.wordIds ?? []) pushRelation(sentence.id, wordId, "used-in", 1);
  }

  // 题库：训练题库 + 测评题（两套 id 不重叠），按 id 去重
  const exercises = [];
  const exercise_entities = [];
  const seenExercises = new Set();
  const addExercise = (exercise) => {
    if (!exercise?.id || seenExercises.has(exercise.id)) return;
    seenExercises.add(exercise.id);

    const payload = {};
    for (const [key, value] of Object.entries(exercise)) {
      if (EXERCISE_BASE_KEYS.has(key)) continue;
      payload[key] = value;
    }

    exercises.push({
      id: exercise.id,
      kind: exercise.kind,
      difficulty: exercise.difficulty,
      xp: exercise.xp ?? 0,
      instruction: exercise.prompt?.instruction ?? "",
      context: orNull(exercise.prompt?.context),
      explanation: exercise.explanation ?? "",
      tags_json: json(exercise.tags),
      payload_json: JSON.stringify(payload),
    });

    for (const entityId of exercise.entityIds ?? []) {
      if (!entityIds.has(entityId)) continue;
      exercise_entities.push({ exercise_id: exercise.id, entity_id: entityId });
    }
  };

  for (const kind of EXERCISE_KINDS) {
    for (const exercise of exercisesModule.exercisesOf(kind)) addExercise(exercise);
  }

  const assessment_modules = [];
  const assessment_sections = [];
  const assessment_section_exercises = [];

  assessmentModules.forEach((module, moduleIndex) => {
    assessment_modules.push({
      id: module.id,
      title: module.title,
      subtitle: module.subtitle,
      description: module.description,
      minutes: module.minutes,
      sort_order: moduleIndex,
    });
    module.sections.forEach((section, sectionIndex) => {
      assessment_sections.push({
        id: section.id,
        module_id: module.id,
        title: section.title,
        description: section.description,
        kind: section.kind,
        skill: section.skill,
        sort_order: sectionIndex,
        // 分项角色：有题目=可作答分项；无题目=汇总/评估分项（如口语 sec_speak_eval）。
        // 显式落库，避免"有没有题"成为隐式约定。
        section_role: section.items.length > 0 ? "graded" : "summary",
      });
      section.items.forEach((item, itemIndex) => {
        addExercise(item);
        assessment_section_exercises.push({
          section_id: section.id,
          exercise_id: item.id,
          sort_order: itemIndex,
        });
      });
    });
  });

  const training_options = exercisesModule.trainingOptions.map((option, index) => ({
    id: option.id,
    track: option.track,
    kind: option.kind,
    title: option.title,
    description: option.description,
    icon: option.icon,
    minutes: option.minutes,
    item_count: option.itemCount,
    difficulty: option.difficulty,
    sort_order: index,
  }));

  const tables = {
    entities,
    vocabulary,
    word_meanings,
    word_examples,
    grammar_points,
    grammar_examples,
    grammar_mistakes,
    sentences: sentenceRows,
    knowledge_relations,
    training_options,
    exercises,
    exercise_entities,
    assessment_modules,
    assessment_sections,
    assessment_section_exercises,
  };

  const digest = createHash("sha256").update(JSON.stringify(tables)).digest("hex").slice(0, 12);

  return { version: `pack-${digest}`, tables };
}

const pack = buildPack();
const payload = `${JSON.stringify(pack, null, 2)}\n`;

if (process.argv.includes("--check")) {
  if (!existsSync(OUT)) {
    console.error(`[content] 缺少 ${OUT}，请先运行 npm run export:content`);
    process.exit(1);
  }
  const current = JSON.parse(readFileSync(OUT, "utf8"));
  const same =
    current.version === pack.version &&
    JSON.stringify(current.tables) === JSON.stringify(pack.tables);
  if (!same) {
    console.error("[content] 内容包与 TS 内容不一致，请运行 npm run export:content");
    process.exit(1);
  }
  console.log(`[content] 校验通过：${pack.version}`);
  process.exit(0);
}

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, payload, "utf8");

const counts = Object.entries(pack.tables).map(([name, rows]) => `${name}=${rows.length}`).join(" ");
console.log(`[content] 已生成 ${OUT}\n[content] version=${pack.version}\n[content] ${counts}`);
