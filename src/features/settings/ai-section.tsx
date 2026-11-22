import { useCallback, useEffect, useState } from "react";
import { Bot, Loader2, PlugZap, Trash2 } from "lucide-react";
import { toast } from "sonner";
import type { AiProviderView, AiStatus } from "@/types/ai";
import { aiService, dataSource } from "@/services/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { LoadingState } from "@/components/common/states";

const STATUS_META: Record<string, { label: string; hint: string }> = {
  uninitialized: { label: "未初始化", hint: "尚未读取配置" },
  initializing: { label: "初始化中", hint: "正在加载 Provider" },
  ready: { label: "正常运行", hint: "配置已生效" },
  error: { label: "异常", hint: "初始化失败，可重新保存配置" },
  shutdown: { label: "已关闭", hint: "应用退出时置位" },
};

interface Draft {
  id: string;
  displayName: string;
  protocol: string;
  baseUrl: string;
  defaultModel: string;
  models: string;
  apiKey: string;
  enabled: boolean;
}

const EMPTY_DRAFT: Draft = {
  id: "",
  displayName: "",
  protocol: "openai-compatible",
  baseUrl: "",
  defaultModel: "",
  models: "",
  apiKey: "",
  enabled: true,
};

export function AiSection() {
  const [status, setStatus] = useState<AiStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);

  const load = useCallback(async () => {
    const response = await aiService.getStatus();
    setLoading(false);
    if (response.ok) {
      setStatus(response.data);
      setError(null);
    } else {
      setStatus(null);
      setError(response.error.detail ?? response.error.message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const editProvider = (provider: AiProviderView) => {
    setDraft({
      id: provider.id,
      displayName: provider.displayName,
      protocol: provider.protocol,
      baseUrl: provider.baseUrl,
      defaultModel: provider.defaultModel ?? "",
      models: provider.models.join(", "),
      // 密钥永不回传，留空表示不修改
      apiKey: "",
      enabled: provider.enabled,
    });
  };

  const saveProvider = async () => {
    if (!draft.id.trim()) {
      toast.error("请先填写 Provider ID");
      return;
    }
    setSaving(true);
    const response = await aiService.upsertProvider({
      id: draft.id.trim(),
      displayName: draft.displayName.trim(),
      protocol: draft.protocol,
      baseUrl: draft.baseUrl.trim(),
      defaultModel: draft.defaultModel.trim() || null,
      models: draft.models
        .split(",")
        .map((item) => item.trim())
        .filter(Boolean),
      enabled: draft.enabled,
      // 空字符串表示"不修改"，因此只在用户输入时才传
      apiKey: draft.apiKey.trim() ? draft.apiKey.trim() : undefined,
    });
    setSaving(false);

    if (response.ok) {
      toast.success("AI 配置已保存");
      setDraft({ ...EMPTY_DRAFT, id: response.data.id });
      void load();
    } else {
      toast.error("AI 配置没有保存成功", { description: response.error.detail });
    }
  };

  const deleteProvider = async (providerId: string) => {
    const response = await aiService.deleteProvider(providerId);
    if (response.ok) {
      toast.success("已删除该 Provider 与其密钥");
      if (draft.id === providerId) setDraft(EMPTY_DRAFT);
      void load();
    } else {
      toast.error("删除失败", { description: response.error.detail });
    }
  };

  const testProvider = async (providerId?: string) => {
    setTesting(true);
    const response = await aiService.testProvider(providerId);
    setTesting(false);
    if (!response.ok) {
      toast.error("连接探测未完成", { description: response.error.detail });
      return;
    }
    const result = response.data;
    if (result.ok) {
      toast.success(`连接正常 · ${result.latencyMs} ms`, {
        description: `${result.providerId} / ${result.model}`,
      });
    } else {
      toast.error("连接失败", { description: result.message ?? "Provider 未返回内容" });
    }
  };

  return (
    <section id="ai" className="scroll-mt-4">
      <div className="mb-3 flex items-start gap-2.5">
        <Bot className="mt-0.5 size-4 shrink-0 text-ink-muted" />
        <div>
          <h2 className="type-section-title text-ink">AI 集成</h2>
          <p className="mt-0.5 text-2xs text-ink-muted">
            SAGE Intelligence 随客户端一起编译（Library 模式）。密钥只保存在本机后端，不会进入前端页面。
          </p>
        </div>
      </div>

      <div className="space-y-4">
        <div className="rounded-md border border-line bg-surface p-4">
          {loading ? (
            <LoadingState variant="text" />
          ) : status ? (
            <>
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div className="flex items-center gap-2">
                  <span className="text-[13.5px] font-medium text-ink">SAGE Intelligence</span>
                  <Badge variant="outline" className="font-mono">
                    v{status.version.intelligenceVersion}
                  </Badge>
                  <Badge variant="outline" className="font-mono">
                    {status.version.integrationMode}
                  </Badge>
                  <Badge variant="outline" className="font-mono">
                    {`Sage v${status.version.clientVersion}`}
                  </Badge>
                </div>
                <Badge variant="outline">
                  {STATUS_META[status.status]?.label ?? status.status}
                </Badge>
              </div>
              <Separator className="my-3" />
              <dl className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                <InfoItem label="框架提交" value={status.version.intelligenceCommit.slice(0, 7)} />
                <InfoItem label="集成协议" value={`v${status.version.protocolVersion}`} />
                <InfoItem label="构建平台" value={status.version.platform} />
                <InfoItem label="构建时间" value={status.version.buildTime} />
              </dl>
              <Separator className="my-3" />
              <dl className="grid gap-2 sm:grid-cols-3">
                <InfoItem label="默认 Provider" value={status.defaultProvider ?? "未配置"} />
                <InfoItem label="默认模型" value={status.defaultModel ?? "未配置"} />
                <InfoItem label="活跃请求" value={`${status.activeStreams}`} />
              </dl>
              {(status.lastError || status.startedAt) && (
                <>
                  <Separator className="my-3" />
                  <dl className="grid gap-2 sm:grid-cols-2">
                    <InfoItem label="最近启动" value={status.startedAt ?? "—"} />
                    <InfoItem label="最近错误" value={status.lastError ?? "无"} />
                  </dl>
                </>
              )}
            </>
          ) : (
            <p className="text-2xs leading-relaxed text-ink-muted">
              {dataSource === "mock"
                ? "当前为本地模拟模式，AI 能力需要桌面端 Rust 后端。切换到 Tauri 后端后这里会显示真实状态。"
                : `AI 服务不可用：${error ?? "未知原因"}`}
            </p>
          )}
        </div>

        {status && (
          <div className="rounded-md border border-line bg-surface p-4">
            <div className="mb-3 flex items-center justify-between gap-3">
              <span className="text-[13px] font-medium text-ink">已配置 Provider</span>
              <Button
                size="sm"
                variant="outline"
                onClick={() => void testProvider(status.defaultProvider ?? undefined)}
                disabled={testing || status.providers.length === 0}
              >
                {testing ? <Loader2 className="animate-spin" /> : <PlugZap />}
                测试默认连接
              </Button>
            </div>

            {status.providers.length === 0 ? (
              <p className="text-2xs text-ink-muted">
                还没有 Provider。填写下方表单后保存，即可用于 AI 请求。
              </p>
            ) : (
              <ul className="space-y-2">
                {status.providers.map((provider) => (
                  <li
                    key={provider.id}
                    className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-line bg-sunken px-3 py-2"
                  >
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-[13px] text-ink">{provider.displayName}</span>
                        <span className="font-mono text-2xs text-ink-muted">{provider.id}</span>
                        {status.defaultProvider === provider.id && (
                          <Badge variant="outline">默认</Badge>
                        )}
                        {!provider.enabled && <Badge variant="outline">已停用</Badge>}
                      </div>
                      <p className="mt-0.5 text-2xs text-ink-muted">
                        {provider.protocol} · {provider.baseUrl} ·{" "}
                        {provider.defaultModel ?? "未设置默认模型"}
                        {provider.hasApiKey ? " · 已配置密钥" : " · 待填密钥"}
                      </p>
                    </div>
                    <div className="flex shrink-0 items-center gap-1.5">
                      <Button size="sm" variant="ghost" onClick={() => editProvider(provider)}>
                        编辑
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => void testProvider(provider.id)}
                        disabled={testing}
                      >
                        测试
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => void deleteProvider(provider.id)}
                      >
                        <Trash2 />
                      </Button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}

        <div className="rounded-md border border-line bg-surface p-4">
          <div className="grid gap-4 sm:grid-cols-2">
            <div>
              <Label className="mb-1.5 block">Provider ID</Label>
              <Input
                value={draft.id}
                placeholder="deepseek"
                onChange={(event) => setDraft({ ...draft, id: event.target.value })}
              />
            </div>
            <div>
              <Label className="mb-1.5 block">显示名称</Label>
              <Input
                value={draft.displayName}
                placeholder="DeepSeek"
                onChange={(event) => setDraft({ ...draft, displayName: event.target.value })}
              />
            </div>
            <div>
              <Label className="mb-1.5 block">协议</Label>
              <Select
                value={draft.protocol}
                onValueChange={(value) => setDraft({ ...draft, protocol: value })}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="openai-compatible">OpenAI 兼容</SelectItem>
                  <SelectItem value="anthropic">Anthropic</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div>
              <Label className="mb-1.5 block">Base URL</Label>
              <Input
                value={draft.baseUrl}
                placeholder="https://api.deepseek.com"
                onChange={(event) => setDraft({ ...draft, baseUrl: event.target.value })}
              />
            </div>
            <div>
              <Label className="mb-1.5 block">默认模型</Label>
              <Input
                value={draft.defaultModel}
                placeholder="deepseek-chat"
                onChange={(event) => setDraft({ ...draft, defaultModel: event.target.value })}
              />
            </div>
            <div>
              <Label className="mb-1.5 block">可用模型（逗号分隔）</Label>
              <Input
                value={draft.models}
                placeholder="deepseek-chat, deepseek-reasoner"
                onChange={(event) => setDraft({ ...draft, models: event.target.value })}
              />
            </div>
            <div className="sm:col-span-2">
              <Label className="mb-1.5 block">API Key</Label>
              <Input
                type="password"
                value={draft.apiKey}
                placeholder={draft.id ? "留空表示不修改已保存的密钥" : "sk-..."}
                onChange={(event) => setDraft({ ...draft, apiKey: event.target.value })}
              />
              <p className="mt-1 text-2xs text-ink-muted">
                密钥写入本机应用数据目录的独立文件，不进入学习数据库，也不会回传到页面。
              </p>
            </div>
          </div>

          <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <Switch
                checked={draft.enabled}
                onCheckedChange={(checked) => setDraft({ ...draft, enabled: checked })}
              />
              <span className="text-2xs text-ink-muted">启用该 Provider</span>
            </div>
            <div className="flex items-center gap-2">
              <Button variant="ghost" size="sm" onClick={() => setDraft(EMPTY_DRAFT)}>
                清空
              </Button>
              <Button size="sm" onClick={saveProvider} disabled={saving}>
                {saving && <Loader2 className="animate-spin" />}
                保存配置
              </Button>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

function InfoItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <dt className="text-2xs text-ink-muted">{label}</dt>
      <dd className="truncate font-mono text-[12.5px] text-ink" title={value}>
        {value}
      </dd>
    </div>
  );
}
