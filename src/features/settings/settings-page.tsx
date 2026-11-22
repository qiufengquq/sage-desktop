import { useEffect, useState } from "react";
import {
  BellRing,
  Bot,
  CloudCheck,
  Database,
  Info,
  Loader2,
  MonitorSmartphone,
  Palette,
  Target,
  Trash2,
  UserRound,
} from "lucide-react";
import { AiSection } from "./ai-section";
import { toast } from "sonner";
import type { AppSettings, LocalDataSummary, SyncStatus, UserProfile } from "@/types";
import { dataSource, isTauriRuntime, userService } from "@/services/api";
import { useQuery } from "@/hooks/use-query";
import { useUiStore } from "@/app/store/ui-store";
import { formatRelativeTime } from "@/lib/date";
import { cn } from "@/lib/utils";
import { PageContainer } from "@/components/common/page-container";
import { PageHeader } from "@/components/common/page-header";
import { ErrorState, LoadingState } from "@/components/common/states";
import { ConfirmDialog } from "@/components/common/confirm-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const SECTIONS = [
  { id: "profile", label: "账户与目标", icon: UserRound },
  { id: "goal", label: "学习节奏", icon: Target },
  { id: "reminder", label: "学习提醒", icon: BellRing },
  { id: "appearance", label: "外观与主题", icon: Palette },
  { id: "sync", label: "数据同步", icon: CloudCheck },
  { id: "local", label: "本地数据", icon: Database },
  { id: "ai", label: "AI 集成", icon: Bot },
  { id: "about", label: "关于", icon: Info },
];

export function SettingsPage() {
  const profileQuery = useQuery((options) => userService.getProfile(options), []);
  const settingsQuery = useQuery((options) => userService.getSettings(options), []);
  const syncQuery = useQuery((options) => userService.getSyncStatus(options), []);
  const localQuery = useQuery((options) => userService.getLocalData(options), []);

  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [sync, setSync] = useState<SyncStatus | null>(null);
  const [local, setLocal] = useState<LocalDataSummary | null>(null);
  const [saving, setSaving] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);

  const theme = useUiStore((s) => s.theme);
  const setTheme = useUiStore((s) => s.setTheme);
  const accent = useUiStore((s) => s.accent);
  const setAccent = useUiStore((s) => s.setAccent);
  const density = useUiStore((s) => s.density);
  const setDensity = useUiStore((s) => s.setDensity);

  useEffect(() => {
    if (profileQuery.data) setProfile(profileQuery.data);
  }, [profileQuery.data]);
  useEffect(() => {
    if (settingsQuery.data) setSettings(settingsQuery.data);
  }, [settingsQuery.data]);
  useEffect(() => {
    if (syncQuery.data) setSync(syncQuery.data);
  }, [syncQuery.data]);
  useEffect(() => {
    if (localQuery.data) setLocal(localQuery.data);
  }, [localQuery.data]);

  const saveProfile = async () => {
    if (!profile) return;
    setSaving(true);
    const response = await userService.updateProfile(profile);
    setSaving(false);
    if (response.ok) {
      setProfile(response.data);
      toast.success("已保存个人资料");
    }
  };

  const saveSettings = async (patch: Partial<AppSettings>) => {
    if (!settings) return;
    const next = { ...settings, ...patch };
    setSettings(next);
    const response = await userService.updateSettings(patch);
    if (!response.ok) toast.error("设置没有保存成功，请重试");
  };

  const triggerSync = async () => {
    setSyncing(true);
    const response = await userService.triggerSync();
    setSyncing(false);
    if (response.ok) {
      setSync(response.data);
      toast.success("同步完成");
    } else {
      toast.error("云同步尚未接入", { description: response.error.message });
    }
  };

  const clearLocal = async () => {
    const response = await userService.clearLocalData();
    if (response.ok) {
      toast.success("本地学习记录已清理", {
        description: "训练记录、测评成绩、复习日志与掌握度已重置，知识库内容保留。",
      });
    }
  };

  /**
   * 分区跳转必须走脚本滚动，不能改 `location.hash`：
   * 应用使用 HashRouter，`#profile` 会被解析成路由 `/profile`，直接掉进 404 兜底页。
   */
  const jumpToSection = (id: string) => {
    const target = document.getElementById(id);
    if (!target) return;
    target.scrollIntoView({ behavior: "smooth", block: "start" });
    // 让键盘与读屏焦点跟随跳转（section 默认不可聚焦，临时补一个 tabindex）
    if (!target.hasAttribute("tabindex")) target.setAttribute("tabindex", "-1");
    target.focus({ preventScroll: true });
  };

  if (profileQuery.initial || settingsQuery.initial) {
    return (
      <PageContainer>
        <PageHeader title="设置" description="正在读取本地设置…" />
        <LoadingState variant="text" />
      </PageContainer>
    );
  }

  if (profileQuery.error) {
    return (
      <PageContainer>
        <PageHeader title="设置" />
        <ErrorState error={profileQuery.error} onRetry={profileQuery.reload} />
      </PageContainer>
    );
  }

  return (
    <PageContainer>
      <PageHeader
        title="设置"
        description="资料、节奏、外观与学习数据都保存在本机。云同步尚未接入，相关接口已按真实契约预留。"
        meta={
          <Badge variant="outline" className="font-mono">
            数据源：{dataSource === "mock" ? "本地模拟" : "Tauri 后端"}
          </Badge>
        }
      />

      <div className="grid gap-8 xl:grid-cols-[11rem_minmax(0,1fr)]">
        <nav className="hidden xl:block" aria-label="设置分区">
          <ul className="sticky top-2 space-y-0.5">
            {SECTIONS.map((section) => {
              const Icon = section.icon;
              return (
                <li key={section.id}>
                  <button
                    type="button"
                    onClick={() => jumpToSection(section.id)}
                    className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-2xs text-ink-muted transition-colors hover:bg-sunken hover:text-ink"
                  >
                    <Icon className="size-3.5" />
                    {section.label}
                  </button>
                </li>
              );
            })}
          </ul>
        </nav>

        <div className="min-w-0 space-y-9">
          {/* 账户与目标 */}
          <section id="profile" className="scroll-mt-4">
            <SectionTitle icon={UserRound} title="账户与目标" description="用于个性化计划与难度推算" />
            {profile && (
              <div className="rounded-md border border-line bg-surface p-4">
                <div className="grid gap-4 sm:grid-cols-2">
                  <Field label="显示名称">
                    <Input
                      value={profile.displayName}
                      onChange={(event) =>
                        setProfile({ ...profile, displayName: event.target.value })
                      }
                    />
                  </Field>
                  <Field label="用户名">
                    <Input
                      value={profile.handle}
                      onChange={(event) => setProfile({ ...profile, handle: event.target.value })}
                    />
                  </Field>
                  <Field label="当前水平">
                    <Select
                      value={profile.level}
                      onValueChange={(value) =>
                        setProfile({ ...profile, level: value as UserProfile["level"] })
                      }
                    >
                      <SelectTrigger className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {["A1", "A2", "B1", "B2", "C1", "C2"].map((level) => (
                          <SelectItem key={level} value={level}>
                            {level}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </Field>
                  <Field label="目标水平">
                    <Select
                      value={profile.targetLevel}
                      onValueChange={(value) =>
                        setProfile({ ...profile, targetLevel: value as UserProfile["level"] })
                      }
                    >
                      <SelectTrigger className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {["A2", "B1", "B2", "C1", "C2"].map((level) => (
                          <SelectItem key={level} value={level}>
                            {level}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </Field>
                  <Field label="学习目标" className="sm:col-span-2">
                    <Select
                      value={profile.goal}
                      onValueChange={(value) =>
                        setProfile({ ...profile, goal: value as UserProfile["goal"] })
                      }
                    >
                      <SelectTrigger className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="exam">考试（雅思 / 托福 / 四六级）</SelectItem>
                        <SelectItem value="career">职业发展与工作沟通</SelectItem>
                        <SelectItem value="travel">旅行与日常沟通</SelectItem>
                        <SelectItem value="academic">学术阅读与写作</SelectItem>
                        <SelectItem value="interest">兴趣驱动</SelectItem>
                      </SelectContent>
                    </Select>
                  </Field>
                </div>
                <div className="mt-4 flex items-center gap-2">
                  <Button onClick={saveProfile} disabled={saving}>
                    {saving && <Loader2 className="animate-spin" />}
                    保存资料
                  </Button>
                  <span className="text-2xs text-ink-muted">
                    加入于 {formatRelativeTime(profile.joinedAt)}
                  </span>
                </div>
              </div>
            )}
          </section>

          {/* 学习节奏 */}
          <section id="goal" className="scroll-mt-4">
            <SectionTitle
              icon={Target}
              title="学习节奏"
              description="每日目标会直接影响计划里的任务量与建议难度"
            />
            {profile && (
              <div className="space-y-4 rounded-md border border-line bg-surface p-4">
                <div>
                  <div className="flex items-baseline justify-between">
                    <Label>每日学习时间</Label>
                    <span className="font-mono text-2xs text-ink-soft">
                      {profile.dailyMinutes} 分钟
                    </span>
                  </div>
                  <Slider
                    className="mt-2.5"
                    value={[profile.dailyMinutes]}
                    min={10}
                    max={120}
                    step={5}
                    onValueChange={(next) =>
                      setProfile({ ...profile, dailyMinutes: next[0] ?? profile.dailyMinutes })
                    }
                    aria-label="每日学习时间"
                  />
                </div>
                <Separator />
                <div>
                  <div className="flex items-baseline justify-between">
                    <Label>每周学习天数</Label>
                    <span className="font-mono text-2xs text-ink-soft">
                      {profile.weeklyDays} 天
                    </span>
                  </div>
                  <Slider
                    className="mt-2.5"
                    value={[profile.weeklyDays]}
                    min={1}
                    max={7}
                    step={1}
                    onValueChange={(next) =>
                      setProfile({ ...profile, weeklyDays: next[0] ?? profile.weeklyDays })
                    }
                    aria-label="每周学习天数"
                  />
                </div>
                <Button size="sm" onClick={saveProfile} disabled={saving}>
                  保存节奏
                </Button>
              </div>
            )}
          </section>

          {/* 提醒 */}
          <section id="reminder" className="scroll-mt-4">
            <SectionTitle icon={BellRing} title="学习提醒" description="桌面通知会在本地时间触发" />
            {settings && (
              <div className="space-y-3 rounded-md border border-line bg-surface p-4">
                <Row
                  label="开启每日提醒"
                  description="在设定时间提醒你完成当天的最小任务"
                >
                  <Switch
                    checked={settings.reminder.enabled}
                    onCheckedChange={(checked) =>
                      saveSettings({ reminder: { ...settings.reminder, enabled: checked } })
                    }
                    aria-label="开启每日提醒"
                  />
                </Row>
                <Separator />
                <Row label="提醒时间" description="24 小时制">
                  <Input
                    type="time"
                    value={settings.reminder.time}
                    onChange={(event) =>
                      saveSettings({
                        reminder: { ...settings.reminder, time: event.target.value },
                      })
                    }
                    className="h-8 w-28"
                    aria-label="提醒时间"
                  />
                </Row>
                <Separator />
                <Row label="仅工作日提醒" description="周末不打扰">
                  <Switch
                    checked={settings.reminder.weekdaysOnly}
                    onCheckedChange={(checked) =>
                      saveSettings({ reminder: { ...settings.reminder, weekdaysOnly: checked } })
                    }
                    aria-label="仅工作日提醒"
                  />
                </Row>
              </div>
            )}
          </section>

          {/* 外观 */}
          <section id="appearance" className="scroll-mt-4">
            <SectionTitle icon={Palette} title="外观与主题" description="即时生效，保存在本地" />
            <div className="space-y-4 rounded-md border border-line bg-surface p-4">
              <div>
                <Label>主题</Label>
                <div className="mt-2 flex gap-2" role="group" aria-label="主题选择">
                  {(
                    [
                      { value: "light", label: "浅色" },
                      { value: "dark", label: "深色" },
                      { value: "system", label: "跟随系统" },
                    ] as const
                  ).map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() => setTheme(option.value)}
                      aria-pressed={theme === option.value}
                      className={cn(
                        "rounded-md border px-3 py-2 text-2xs transition-colors",
                        theme === option.value
                          ? "border-brand/50 bg-brand-tint text-brand"
                          : "border-line bg-surface text-ink-muted hover:border-line-strong",
                      )}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </div>

              <Separator />

              <Row label="强调色" description="铜色用于连续学习与关键数据">
                <div className="flex gap-1.5">
                  {(
                    [
                      { value: "brand", label: "墨绿", color: "var(--brand)" },
                      { value: "ember", label: "铜橙", color: "var(--ember)" },
                    ] as const
                  ).map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() => setAccent(option.value)}
                      aria-pressed={accent === option.value}
                      className={cn(
                        "flex items-center gap-1.5 rounded-sm border px-2 py-1 text-2xs",
                        accent === option.value
                          ? "border-brand/50 bg-brand-tint/60 text-ink"
                          : "border-line text-ink-muted",
                      )}
                    >
                      <span
                        className="size-2.5 rounded-full"
                        style={{ backgroundColor: option.color }}
                      />
                      {option.label}
                    </button>
                  ))}
                </div>
              </Row>

              <Separator />

              <Row label="界面密度" description="紧凑模式适合 13 寸屏幕">
                <div className="flex gap-1.5">
                  {(
                    [
                      { value: "comfortable", label: "舒适" },
                      { value: "compact", label: "紧凑" },
                    ] as const
                  ).map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() => setDensity(option.value)}
                      aria-pressed={density === option.value}
                      className={cn(
                        "rounded-sm border px-2 py-1 text-2xs",
                        density === option.value
                          ? "border-brand/50 bg-brand-tint/60 text-ink"
                          : "border-line text-ink-muted",
                      )}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </Row>
            </div>
          </section>

          {/* 同步 */}
          <section id="sync" className="scroll-mt-4">
            <SectionTitle
              icon={CloudCheck}
              title="数据同步"
              description="云同步尚未接入，当前所有数据只保存在本机"
            />
            <div className="rounded-md border border-line bg-surface p-4">
              {sync ? (
                <>
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div className="flex items-center gap-2">
                      <Badge variant="outline">
                        {sync.state === "synced" ? "已同步" : "仅本机保存"}
                      </Badge>
                      <span className="text-2xs text-ink-muted">
                        {sync.lastSyncedAt
                          ? `上次同步 ${formatRelativeTime(sync.lastSyncedAt)}`
                          : "从未同步到云端"}
                      </span>
                    </div>
                    <Button size="sm" variant="outline" onClick={triggerSync} disabled={syncing}>
                      {syncing && <Loader2 className="animate-spin" />}
                      立即同步
                    </Button>
                  </div>
                  <Separator className="my-3" />
                  <dl className="grid gap-2 sm:grid-cols-3">
                    <SyncItem label="设备" value={sync.deviceName} />
                    <SyncItem label="待同步变更" value={`${sync.pendingChanges} 项`} />
                    <SyncItem label="本地占用" value={`${sync.storageUsedKb} KB`} />
                  </dl>
                </>
              ) : (
                <LoadingState variant="text" />
              )}
            </div>
          </section>

          {/* 本地数据 */}
          <section id="local" className="scroll-mt-4">
            <SectionTitle icon={Database} title="本地数据" description="所有学习记录都在本机，不上传原文" />
            <div className="rounded-md border border-line bg-surface p-4">
              {local ? (
                <>
                  <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                    <SyncItem label="词条" value={`${local.words}`} />
                    <SyncItem label="语法点" value={`${local.grammarPoints}`} />
                    <SyncItem label="句子" value={`${local.sentences}`} />
                    <SyncItem label="学习日" value={`${local.records}`} />
                  </dl>
                  <Separator className="my-3" />
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <span className="text-2xs text-ink-muted">
                      数据库占用约 {local.sizeKb} KB · 路径由 Rust 侧管理
                    </span>
                    <Button variant="outline" size="sm" onClick={() => setConfirmClear(true)}>
                      <Trash2 />
                      清理本地记录
                    </Button>
                  </div>
                </>
              ) : (
                <LoadingState variant="text" />
              )}
            </div>
          </section>

          {/* AI 集成 */}
          <AiSection />

          {/* 关于 */}
          <section id="about" className="scroll-mt-4">
            <SectionTitle icon={Info} title="关于 Sage" description={`桌面正式版 ${__APP_VERSION__}`} />
            <div className="rounded-md border border-line bg-surface p-4">
              <div className="flex items-start gap-3">
                <div className="flex size-9 items-center justify-center rounded-sm bg-brand text-brand-ink">
                  <span className="font-display text-base leading-none">S</span>
                </div>
                <div>
                  <div className="text-[13.5px] font-medium text-ink">{`Sage ${__APP_VERSION__}`}</div>
                  <p className="mt-0.5 max-w-[60ch] text-2xs leading-relaxed text-ink-muted">
                    英语智能学习平台。React + TypeScript + Tailwind CSS 4 + shadcn/ui + Tauri 2
                    构建。学习数据保存在本机，所有服务调用经过统一传输层，可切换到 Rust
                    后端实现。
                  </p>
                </div>
              </div>
              <Separator className="my-3" />
              <dl className="grid gap-2 sm:grid-cols-2">
                <SyncItem label="运行环境" value={isTauriRuntime() ? "Tauri 桌面" : "浏览器预览"} />
                <SyncItem
                  label="数据源"
                  value={dataSource === "mock" ? "mock（本地模拟）" : "backend（Rust）"}
                />
                <SyncItem label="设计语言" value="Instrument" />
                <SyncItem label="无障碍" value="键盘焦点可见 · 尊重减少动效" />
              </dl>
              <div className="mt-3 flex items-center gap-1.5 text-2xs text-ink-muted">
                <MonitorSmartphone className="size-3" />
                建议窗口宽度 ≥ 1180px，最小支持 1024px。
              </div>
            </div>
          </section>
        </div>
      </div>

      <ConfirmDialog
        open={confirmClear}
        onOpenChange={setConfirmClear}
        title="清理本地学习记录？"
        description="会删除本机的学习历史与复习队列，知识库内容不受影响。这个操作无法撤销。"
        confirmLabel="清理记录"
        destructive
        onConfirm={() => {
          setConfirmClear(false);
          void clearLocal();
        }}
      />
    </PageContainer>
  );
}

function SectionTitle({
  icon: Icon,
  title,
  description,
}: {
  icon: typeof UserRound;
  title: string;
  description?: string;
}) {
  return (
    <div className="mb-3 flex items-start gap-2.5">
      <Icon className="mt-0.5 size-4 shrink-0 text-ink-muted" />
      <div>
        <h2 className="type-section-title text-ink">{title}</h2>
        {description && <p className="mt-0.5 text-2xs text-ink-muted">{description}</p>}
      </div>
    </div>
  );
}

function Field({
  label,
  children,
  className,
}: {
  label: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={className}>
      <Label className="mb-1.5 block">{label}</Label>
      {children}
    </div>
  );
}

function Row({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div className="min-w-0">
        <div className="text-[13px] text-ink">{label}</div>
        {description && <p className="text-2xs text-ink-muted">{description}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function SyncItem({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-2xs text-ink-muted">{label}</dt>
      <dd className="mt-0.5 font-mono text-2xs text-ink-soft">{value}</dd>
    </div>
  );
}
