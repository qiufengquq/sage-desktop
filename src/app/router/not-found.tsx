import { Link } from "react-router";
import { Compass } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/common/states";
import { PageContainer } from "@/components/common/page-container";

export function NotFoundPage() {
  return (
    <PageContainer>
      <div className="flex min-h-[60vh] items-center justify-center">
        <EmptyState
          icon={Compass}
          title="这个页面不存在"
          description="链接可能已经失效。回到首页继续今天的学习计划。"
          action={
            <Button asChild>
              <Link to="/">回到首页</Link>
            </Button>
          }
          className="max-w-md"
        />
      </div>
    </PageContainer>
  );
}
