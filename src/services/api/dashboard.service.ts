import type { ApiResult, DashboardData, RequestOptions } from "@/types";
import { latency } from "@/services/mock/base";
import { buildDashboardData } from "@/services/data/selectors";
import { getDb } from "@/stores/learning-store";
import { COMMANDS } from "./contracts";
import { serve } from "./transport";

/** 首页数据全部由当前学习状态派生，不再有静态快照 */
export function getDashboard(options: RequestOptions = {}): Promise<ApiResult<DashboardData>> {
  return serve(
    COMMANDS.dashboardSnapshot,
    async () => {
      await latency(options.signal);
      const now = new Date();
      return buildDashboardData(getDb(now), now);
    },
    { request: options },
  );
}
