import { beforeEach, describe, expect, it } from "vitest";

import { createMemoryRepository } from "@/services/data/repository";
import { getDb, resetLearningStore, setLearningRepository } from "@/stores/learning-store";
import { knowledgeService } from "@/services/api";
import { findWord } from "@/services/mock/vocabulary";

/**
 * 2.2.4 的回归护栏：内容实体不再自带学习态字段。
 *
 * 之前 `Vocabulary / GrammarPoint / Sentence` 上各有一份 mastery/dueAt/...，
 * 与 `db.entityStates` 并存，写入只更新后者，于是知识库会显示出永远陈旧的掌握度。
 */
describe("学习态单一事实源", () => {
  beforeEach(() => {
    setLearningRepository(createMemoryRepository());
    resetLearningStore();
  });

  it("内容实体上不存在学习态字段", () => {
    const word = findWord("w_ubiquitous");
    expect(word).toBeTruthy();
    for (const field of [
      "mastery",
      "state",
      "reviewCount",
      "dueAt",
      "strength",
      "lastReviewedAt",
    ]) {
      expect(word as unknown as Record<string, unknown>).not.toHaveProperty(field);
    }
  });

  it("知识库详情的掌握度来自 entityStates，并作为独立字段返回", async () => {
    const id = "w_ubiquitous";
    const state = getDb().entityStates[id];
    expect(state).toBeTruthy();

    const detail = await knowledgeService.getKnowledgeDetail(id);
    if (!detail.ok) throw new Error(detail.error.message);
    if (detail.data.kind !== "word" && detail.data.kind !== "phrase") {
      throw new Error("期望单词详情");
    }

    expect(detail.data.state?.mastery).toBe(state.mastery);
    expect(detail.data.state?.dueAt).toBe(state.dueAt);
    // 内容正文与学习态彻底分离
    expect(detail.data.word.id).toBe(id);
    expect(detail.data.word).not.toHaveProperty("mastery");
  });

  it("列表摘要的掌握度与 entityStates 一致", async () => {
    const list = await knowledgeService.getKnowledgeList();
    if (!list.ok) throw new Error(list.error.message);

    const states = getDb().entityStates;
    for (const item of list.data.slice(0, 10)) {
      expect(item.mastery).toBe(states[item.id]?.mastery ?? 0);
      expect(item.state).toBe(states[item.id]?.state ?? "new");
    }
  });
});
