/**
 * 服务层统一出口。
 * 页面与 feature 组件只允许从这里（或具体 *.service）读取数据，
 * 不允许直接 import services/mock/*。
 */
export * as dashboardService from "./dashboard.service";
export * as assessmentService from "./assessment.service";
export * as plannerService from "./planner.service";
export * as analyticsService from "./analytics.service";
export * as knowledgeService from "./knowledge.service";
export * as trainingService from "./training.service";
export * as reviewService from "./review.service";
export * as userService from "./user.service";

export type { ReviewQueuePayload, ReviewSubmitResult } from "./review.service";
export { COMMANDS } from "./contracts";
export type { CommandName } from "./contracts";
export { dataSource, isTauriRuntime, normalizeError, serve } from "./transport";
