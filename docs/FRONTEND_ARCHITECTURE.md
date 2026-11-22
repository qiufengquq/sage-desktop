# 前端架构

## 1. 技术栈与版本

| 能力 | 选型 | 版本 |
|---|---|---|
| UI 框架 | React | 19.3 |
| 语言 | TypeScript | 7.0（strict，`erasableSyntaxOnly`） |
| 构建 | Vite | 8.3 + `@vitejs/plugin-react` |
| 样式 | Tailwind CSS | 4.3（CSS-first，`@theme`） |
| 组件底座 | shadcn/ui 模式（源码内置 + Radix Primitives + CVA） | Radix 1.x/2.x |
| 图标 | lucide-react | 1.73 |
| 路由 | react-router | 8.5（HashRouter） |
| 状态 | zustand | 5.1（含 persist） |
| 桌面外壳 | Tauri | 2.12 |
| 提示 | sonner | 2.0 |
| 字体 | @fontsource-variable（本地打包） | Instrument Sans / Newsreader / JetBrains Mono |

---

## 2. 分层与目录

```
src/
├─ app/                     应用装配层
│  ├─ App.tsx               根组件（Providers + Routes）
│  ├─ providers/            主题桥接、Tooltip、Toaster
│  ├─ router/               路由表与 404
│  └─ store/                zustand（ui-store：主题、侧栏、命令面板）
├─ components/
│  ├─ ui/                   shadcn 基础组件（可继续 `npx shadcn add`）
│  ├─ layout/               AppShell / AppSidebar / AppHeader / CommandPalette
│  ├─ common/               业务无关的复合组件（卡片、状态、录音…）
│  └─ charts/               手绘 SVG 图表
├─ features/                按业务域组织的页面与专用组件
│  ├─ dashboard/  ├─ assessment/  ├─ planner/  ├─ analytics/
│  ├─ knowledge/  ├─ training/    └─ settings/
├─ services/
│  ├─ api/                  页面唯一可见的数据入口（*.service.ts）
│  │   ├─ contracts.ts      命令名常量（与 Rust 一一对应）
│  │   └─ transport.ts      Mock / Tauri 分发、错误归一化
│  └─ mock/                 模拟数据与派生计算（页面不可直接引用）
├─ types/                   跨层共享类型（可辨识联合优先）
├─ hooks/                   useQuery / useThemeSync / useHotkey
├─ lib/                     utils、date、format、grading、speech
└─ styles/globals.css       设计 token（唯一色值来源）

src-tauri/
├─ src/commands/            Rust Command 骨架（app / user / learning / knowledge）
├─ src/error.rs             AppError → "code:message" 序列化
├─ src/state.rs             AppState { db: Arc<Database>, ai: Arc<AiState> }
├─ capabilities/default.json 最小权限
└─ tauri.conf.json          窗口、CSP、打包配置
```

依赖方向严格单向：

```
features → components/common → components/ui
features → services/api → (services/mock | Tauri invoke)
features → types / hooks / lib
```

`features` 之间只允许通过路由跳转协作，不互相 import 组件（唯一例外：测评复用训练的
`ExerciseView`，因为它就是同一套题目渲染器）。

---

## 3. 数据流

```
页面 (feature)
  │  useQuery(service.getXxx)         ← 统一 loading / error / abort
  ▼
services/api/xxx.service.ts
  │  serve(command, mockFn, { args }) ← 返回 ApiResult<T>
  ▼
services/api/transport.ts
  ├─ dataSource = "mock"  → 执行 mockFn（模拟 90–320ms 延迟）
  └─ dataSource = "backend" → invoke(command, args)（需在 Tauri 内运行）
```

关键约定：

1. **页面永不 import `services/mock/*`**。Mock 只被 `services/api` 引用。
2. 服务层返回 `ApiResult<T>`，不抛业务异常；取消请求抛 `AbortError`，由 `useQuery` 忽略。
3. `useQuery` 用 `AbortController` 在卸载/依赖变化时取消，避免竞态与已卸载组件更新。
4. Mock 数据由**同一份事实表**派生：`dailyStats` → 周统计、趋势、时间分布、仪表盘数字。
   不存在两套互相矛盾的假数据。

---

## 4. 状态管理

| 状态 | 位置 | 持久化 |
|---|---|---|
| 主题 / 侧栏折叠 / 强调色 / 密度 | `app/store/ui-store.ts` | `localStorage`（zustand persist, key `sage.ui`） |
| 全局搜索面板开关 | 同上（非持久化） | — |
| 页面数据（列表、详情、统计） | `useQuery` + 服务层 | — |
| 训练会话进度 | `TrainingSessionRunner` / `AssessmentRunner` 局部状态 | — |
| 表单草稿 | 页面局部状态，提交后回写服务层 | — |

不引入全局数据缓存库（如 React Query）是刻意的：当前数据量小、切换数据源后服务层即
后端，缓存策略应由后端与页面共同决定，而不是提前锁定一个客户端缓存语义。

---

## 5. 路由

`HashRouter` + 声明式 `<Routes>`：

| 路径 | 页面 |
|---|---|
| `/` | Dashboard |
| `/assessment` | Assessment |
| `/planner` | AI Planner |
| `/analytics` | Learning Analytics |
| `/knowledge` | Knowledge Base（`?id=` 直接打开某条详情） |
| `/training` | Training（`?kind=&track=&start=1` 直接开始训练） |
| `/settings` | Settings |

选择 Hash 路由的原因：Tauri 生产环境通过自定义协议加载页面，Hash 路由不依赖服务器
回退规则，刷新/深链行为稳定。

跨页联动全部通过 URL 参数完成，因此可以从任何地方（Dashboard 任务、AI 建议、数据页
复习队列）一键跳到对应训练或知识条目。

---

## 6. 主题机制

1. `index.html` 不带 `class="dark"`，由 `useThemeSync` 在首帧后写入 `<html>`。
2. `ui-store.theme` 可为 `light | dark | system`；`system` 通过 `matchMedia` 监听系统变化。
3. 所有颜色来自 `globals.css` 的 CSS 变量；`.dark` 只覆盖变量值，不覆盖组件样式。
4. `prefers-reduced-motion` 在 CSS 层全局降级，无需组件关心。

---

## 7. 错误与加载策略

| 状态 | 组件 | 行为 |
|---|---|---|
| 首次加载 | `LoadingState` | 骨架屏，形态跟随内容（列表/卡片/图表） |
| 刷新中 | 局部 opacity + 控件禁用 | 不闪回骨架 |
| 请求失败 | `ErrorState` | 说明问题 + 可选重试；技术细节折叠显示 |
| 空结果 | `EmptyState` | 说明为什么为空 + 一个可执行动作 |
| 提交失败 | `sonner` toast 或行内错误 | 文案指明「出了什么问题」而非道歉 |

错误码与用户文案的映射集中在 `transport.ts::toUserMessage`，Rust 侧只需返回
`"code:message"` 字符串。

---

## 8. 性能

- 生产构建体积目前约 535 KB（gzip 171 KB），全部页面同步加载。若后续页面继续增大，
  按 `features/*` 做 `React.lazy` 分割即可，路由表是唯一改动点。
- 图表为纯 SVG，无第三方运行时；力导向布局只在进入知识库时计算一次（160 次迭代，
  约 36 个节点）。
- 字体使用 `@fontsource-variable` 的 latin 子集，桌面端离线可用。

---

## 9. 与 Rust 的边界

```
前端职责                          后端职责（第二阶段）
──────────────────────────────    ─────────────────────────────
渲染、交互、即时判分反馈           权威判分与掌握度写回
乐观 UI、局部草稿状态              SQLite 持久化、事务、迁移
                     ┌──────────
                     │ Command 契约（services/api/contracts.ts）
                     │ 入参/出参均为 serde_json::Value 或强类型结构
                     ▼
                             AI Core（本地模型或远端 API）
                             返回结构化 JSON → 前端 schema 校验后渲染
```

硬性约束：**AI 不允许返回可执行的 React/JS 代码**。AI 只产出经过定义的结构化数据
（计划项、建议、题目），前端用固定的组件渲染。

### 9.1 AI 集成（1.0.1）

SAGE Intelligence 以 **Rust Library** 形式随客户端编译，不存在第二个进程：

```
页面（settings/ai-section）
  │  aiService.getStatus / upsertProvider / complete / startStream / cancelStream
  ▼
services/api/ai.service.ts        ← 前端唯一 AI 入口
  │  serve(ai_*, ...)：非 Tauri 环境直接返回 unavailable（不伪造结果）
  ▼
Tauri Command（src-tauri/src/ai/commands.rs）
  │  ai_status / ai_provider_* / ai_complete / ai_stream_* / ai_cancel
  ▼
OutputEngine（ai-core）→ ProviderRuntime → Provider（HTTP）
  │
  └─ 事件：ai-core://*  → AppHandle::emit → aiService.subscribe()
           sage-ai://stream-finished（客户端终态摘要）
```

约束：

1. 页面不得直接 `listen` 框架事件，必须经过 `aiService.subscribe()`。
2. 流式只走事件，不把增量塞进一次 `ApiResult`。
3. API Key 只进不出：命令入参可以带密钥，出参只有 `hasApiKey`。
4. AI 存储与业务库分离：配置在 `ai_providers`，密钥在独立文件，框架数据在 `ai-core-data/`。

---

## 10. 第二阶段演进路径

1. `src-tauri` 接入 SQLite（`sqlx` 或 `rusqlite`），把 `state.rs` 的 KV 换成真实仓储。
2. 逐条实现 `commands/learning.rs` 中的占位命令，前端只需把 `.env` 改成
   `VITE_SAGE_DATA_SOURCE=backend`。
3. 用 Rust 复用 `src/lib/grading.ts` 的判分规则，保证两端结果一致（现有实现即对照标准）。
4. 语音：`lib/speech.ts` 的 `speak/record` 换成 Rust 侧 TTS/ASR 命令。
5. 同步：`sync_trigger` 落地为增量 diff + 冲突合并，前端 UI 已就绪。
