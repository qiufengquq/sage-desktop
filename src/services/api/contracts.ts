/**
 * React ⇄ Rust 的命令契约。
 *
 * 命名规则：`<domain>_<action>`，与 src-tauri/src/commands 中的 #[tauri::command]
 * 函数名一一对应。React 侧只能通过这里出现的命令名访问后端，
 * 便于在 API_CONTRACTS.md 中做一次性审计。
 */
export const COMMANDS = {
  // 用户与设置
  userProfileGet: "user_profile_get",
  userProfileUpdate: "user_profile_update",
  settingsGet: "settings_get",
  settingsUpdate: "settings_update",
  syncStatusGet: "sync_status_get",
  syncTrigger: "sync_trigger",
  localDataSummary: "local_data_summary",
  localDataClear: "local_data_clear",

  // 学习总览
  dashboardSnapshot: "dashboard_snapshot",

  // 测试
  assessmentModules: "assessment_modules",
  assessmentSubmit: "assessment_submit",
  assessmentCheck: "assessment_check",

  // AI 规划
  planGet: "plan_get",
  planRegenerate: "plan_regenerate",
  planApplyDifficulty: "plan_apply_difficulty",
  planToggleItem: "plan_toggle_item",

  // 数据分析
  analyticsQuery: "analytics_query",

  // 知识库
  knowledgeGraph: "knowledge_graph",
  knowledgeList: "knowledge_list",
  knowledgeDetail: "knowledge_detail",
  knowledgeNeighbors: "knowledge_neighbors",

  // 训练
  trainingOptions: "training_options",
  trainingStart: "training_start",
  trainingSubmit: "training_submit",
  trainingCheck: "training_check",

  // 复习（第二阶段新增，Rust 侧在 2B 实现）
  reviewQueue: "review_queue",
  reviewSubmit: "review_submit",
} as const;

export type CommandName = (typeof COMMANDS)[keyof typeof COMMANDS];

/**
 * 命令的**后端实现状态**。
 *
 * 背景：数据源曾经按"是否运行在 Tauri 里"一刀切。结果是打包后只要有一条命令
 * 还没实现，整个页面就会报 `unavailable`。真实需要的是"按命令声明能力"：
 * 已实现的走 Rust/SQLite，未实现的按 mock 回退，页面始终可用。
 *
 * 规则：新增命令时先在这里登记；Rust 侧接线完成后才把 `planned` 改成 `ready`。
 */
export type CommandStatus = "ready" | "planned";

export const COMMAND_STATUS: Record<CommandName, CommandStatus> = {
  // 已接入 SQLite
  user_profile_get: "ready",
  user_profile_update: "ready",
  settings_get: "ready",
  settings_update: "ready",
  sync_status_get: "ready",
  local_data_summary: "ready",
  assessment_modules: "ready",
  assessment_submit: "ready",
  assessment_check: "ready",
  knowledge_graph: "ready",
  knowledge_list: "ready",
  knowledge_detail: "ready",
  knowledge_neighbors: "ready",
  training_options: "ready",
  training_start: "ready",
  training_submit: "ready",
  training_check: "ready",
  review_queue: "ready",
  review_submit: "ready",

  // 尚未实现（Rust 侧返回 unavailable），前端按 mock 回退
  sync_trigger: "planned",
  local_data_clear: "planned",
  dashboard_snapshot: "planned",
  analytics_query: "planned",
  plan_get: "planned",
  plan_regenerate: "planned",
  plan_apply_difficulty: "planned",
  plan_toggle_item: "planned",
};
