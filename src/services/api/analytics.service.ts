import type { AnalyticsData, ApiResult, RequestOptions } from "@/types";
import { latency } from "@/services/mock/base";
import { buildAnalyticsData } from "@/services/data/selectors";
import { getDb } from "@/stores/learning-store";
import { COMMANDS } from "./contracts";
import { serve } from "./transport";

export function getAnalytics(
  range: 7 | 14 | 30 = 30,
  options: RequestOptions = {},
): Promise<ApiResult<AnalyticsData>> {
  return serve(
    COMMANDS.analyticsQuery,
    async () => {
      await latency(options.signal);
      const now = new Date();
      return buildAnalyticsData(getDb(now), range, now);
    },
    { args: { range }, request: options },
  );
}
