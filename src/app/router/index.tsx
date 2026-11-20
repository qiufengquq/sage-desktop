import { lazy, Suspense, type ReactNode } from "react";
import { Route, Routes } from "react-router";
import { AppShell } from "@/components/layout/app-shell";
import { ErrorBoundary } from "@/components/common/error-boundary";
import { LoadingState } from "@/components/common/states";

/**
 * 路由级懒加载：首屏只加载首页，其余模块按需下载。
 * 每个页面独立分包，避免一次加载全部模块。
 */
const DashboardPage = lazy(() =>
  import("@/features/dashboard/dashboard-page").then((module) => ({
    default: module.DashboardPage,
  })),
);
const AssessmentPage = lazy(() =>
  import("@/features/assessment/assessment-page").then((module) => ({
    default: module.AssessmentPage,
  })),
);
const PlannerPage = lazy(() =>
  import("@/features/planner/planner-page").then((module) => ({
    default: module.PlannerPage,
  })),
);
const AnalyticsPage = lazy(() =>
  import("@/features/analytics/analytics-page").then((module) => ({
    default: module.AnalyticsPage,
  })),
);
const KnowledgePage = lazy(() =>
  import("@/features/knowledge/knowledge-page").then((module) => ({
    default: module.KnowledgePage,
  })),
);
const TrainingPage = lazy(() =>
  import("@/features/training/training-page").then((module) => ({
    default: module.TrainingPage,
  })),
);
const ReviewPage = lazy(() =>
  import("@/features/review/review-page").then((module) => ({
    default: module.ReviewPage,
  })),
);
const SettingsPage = lazy(() =>
  import("@/features/settings/settings-page").then((module) => ({
    default: module.SettingsPage,
  })),
);
const NotFoundPage = lazy(() =>
  import("./not-found").then((module) => ({ default: module.NotFoundPage })),
);

/** 只替换内容区域，侧边栏与顶栏保持不动 */
function Page({ children }: { children: ReactNode }) {
  return (
    <Suspense
      fallback={
        <div className="p-6">
          <LoadingState variant="cards" rows={3} />
        </div>
      }
    >
      {children}
    </Suspense>
  );
}

export function AppRoutes() {
  return (
    <ErrorBoundary>
      <Routes>
        <Route element={<AppShell />}>
          <Route
            path="/"
            element={
              <Page>
                <DashboardPage />
              </Page>
            }
          />
          <Route
            path="/assessment"
            element={
              <Page>
                <AssessmentPage />
              </Page>
            }
          />
          <Route
            path="/planner"
            element={
              <Page>
                <PlannerPage />
              </Page>
            }
          />
          <Route
            path="/analytics"
            element={
              <Page>
                <AnalyticsPage />
              </Page>
            }
          />
          <Route
            path="/knowledge"
            element={
              <Page>
                <KnowledgePage />
              </Page>
            }
          />
          <Route
            path="/training"
            element={
              <Page>
                <TrainingPage />
              </Page>
            }
          />
          <Route
            path="/review"
            element={
              <Page>
                <ReviewPage />
              </Page>
            }
          />
          <Route
            path="/settings"
            element={
              <Page>
                <SettingsPage />
              </Page>
            }
          />
          <Route
            path="*"
            element={
              <Page>
                <NotFoundPage />
              </Page>
            }
          />
        </Route>
      </Routes>
    </ErrorBoundary>
  );
}
