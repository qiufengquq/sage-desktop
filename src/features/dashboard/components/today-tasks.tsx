import { ArrowRight } from "lucide-react";
import type { DashboardTask } from "@/types";
import { Button } from "@/components/ui/button";
import { TaskCard } from "@/components/common/task-card";
import { SectionHeader } from "@/components/common/page-header";

export function TodayTasks({
  tasks,
  onStart,
  onStartAll,
}: {
  tasks: DashboardTask[];
  onStart: (task: DashboardTask) => void;
  onStartAll: () => void;
}) {
  const doneCount = tasks.filter((t) => t.status === "done").length;
  const remaining = tasks
    .filter((t) => t.status !== "done")
    .reduce((acc, t) => acc + t.minutes, 0);

  return (
    <section>
      <SectionHeader
        title="今日学习任务"
        description={`已完成 ${doneCount}/${tasks.length} 项 · 还需约 ${remaining} 分钟`}
        action={
          <Button size="sm" variant="ghost" onClick={onStartAll}>
            继续下一项
            <ArrowRight />
          </Button>
        }
      />
      <div className="rounded-md border border-line bg-surface px-4 py-4">
        {tasks.map((task, i) => (
          <TaskCard
            key={task.id}
            task={task}
            index={i}
            isLast={i === tasks.length - 1}
            onStart={onStart}
          />
        ))}
      </div>
    </section>
  );
}
