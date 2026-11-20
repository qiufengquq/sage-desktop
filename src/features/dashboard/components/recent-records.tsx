import { ArrowUpRight } from "lucide-react";
import type { LearningRecord } from "@/types";
import { ACTIVITY_LABEL } from "@/types/learning";
import { formatRelativeTime } from "@/lib/date";
import { SectionHeader } from "@/components/common/page-header";
import { Badge } from "@/components/ui/badge";

export function RecentRecords({ records }: { records: LearningRecord[] }) {
  return (
    <section>
      <SectionHeader title="最近学习记录" description="按时间倒序，点击可在知识库中查看相关条目" />
      <div className="overflow-hidden rounded-md border border-line bg-surface">
        <table className="w-full text-left">
          <thead>
            <tr className="border-b border-line text-2xs text-ink-muted">
              <th className="px-4 py-2 font-medium">内容</th>
              <th className="px-3 py-2 font-medium">类型</th>
              <th className="px-3 py-2 text-right font-medium">时长</th>
              <th className="px-3 py-2 text-right font-medium">正确率</th>
              <th className="px-3 py-2 text-right font-medium">获得经验</th>
              <th className="px-4 py-2 text-right font-medium">时间</th>
            </tr>
          </thead>
          <tbody>
            {records.map((record) => {
              const accuracy =
                record.items === 0 ? 0 : Math.round((record.correct / record.items) * 100);
              return (
                <tr
                  key={record.id}
                  className="border-b border-line/70 last:border-0 transition-colors hover:bg-raised"
                >
                  <td className="max-w-[280px] truncate px-4 py-2.5 text-[13px] text-ink">
                    {record.title}
                  </td>
                  <td className="px-3 py-2.5">
                    <Badge variant="default">{ACTIVITY_LABEL[record.activity]}</Badge>
                  </td>
                  <td className="px-3 py-2.5 text-right font-mono text-2xs text-ink-soft">
                    {record.minutes} 分
                  </td>
                  <td className="px-3 py-2.5 text-right font-mono text-2xs text-ink-soft">
                    <span className={accuracy >= 80 ? "text-success" : accuracy >= 60 ? "text-ink-soft" : "text-danger"}>
                      {accuracy}%
                    </span>
                  </td>
                  <td className="px-3 py-2.5 text-right font-mono text-2xs text-ember">
                    +{record.xp}
                  </td>
                  <td className="px-4 py-2.5 text-right text-2xs whitespace-nowrap text-ink-muted">
                    {formatRelativeTime(record.occurredAt)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <p className="mt-2 flex items-center gap-1 text-2xs text-ink-muted">
        <ArrowUpRight className="size-3" />
        数据来自本地学习记录，共 {records.length} 条
      </p>
    </section>
  );
}
