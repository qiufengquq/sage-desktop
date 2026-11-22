import { describe, expect, it } from "vitest";

import { COMMANDS, COMMAND_STATUS } from "@/services/api/contracts";
import { isBackendReady } from "@/services/api/transport";

/**
 * 数据源曾经按"是否运行在 Tauri 里"一刀切，导致打包后只要有一条命令没实现，
 * 整个页面就报 `unavailable`。这组测试守住"按命令声明能力"的契约。
 */
describe("命令能力声明", () => {
  it("每条命令都必须登记状态", () => {
    for (const name of Object.values(COMMANDS)) {
      expect(COMMAND_STATUS[name], `命令未登记状态：${name}`).toBeDefined();
    }
  });

  it("planned 集合稳定（未实现命令按 mock 回退）", () => {
    const planned = Object.entries(COMMAND_STATUS)
      .filter(([, status]) => status === "planned")
      .map(([name]) => name)
      .sort();

    expect(planned).toEqual(
      [
        "analytics_query",
        "dashboard_snapshot",
        "local_data_clear",
        "plan_apply_difficulty",
        "plan_get",
        "plan_regenerate",
        "plan_toggle_item",
        "sync_trigger",
      ].sort(),
    );
  });

  it("已接线的命令才走后端", () => {
    expect(isBackendReady(COMMANDS.trainingSubmit)).toBe(true);
    expect(isBackendReady(COMMANDS.assessmentSubmit)).toBe(true);
    expect(isBackendReady(COMMANDS.knowledgeList)).toBe(true);
    expect(isBackendReady(COMMANDS.reviewQueue)).toBe(true);

    // 1.0.1：AI 命令已由 Rust 实现（非桌面环境由 service 层返回 unavailable）
    expect(isBackendReady(COMMANDS.aiStatus)).toBe(true);
    expect(isBackendReady(COMMANDS.aiProviderUpsert)).toBe(true);
    expect(isBackendReady(COMMANDS.aiStreamStart)).toBe(true);

    expect(isBackendReady(COMMANDS.dashboardSnapshot)).toBe(false);
    expect(isBackendReady("not_a_command")).toBe(false);
  });
});
