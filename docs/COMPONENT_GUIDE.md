# 组件指南

## 1. 分层原则

| 层 | 位置 | 允许依赖 | 说明 |
|---|---|---|---|
| ui | `components/ui` | 仅 `lib/utils`、Radix | shadcn 基础件，不含业务语义，可 `npx shadcn add` 继续扩展 |
| charts | `components/charts` | ui、lib | 纯 SVG，数据由外部传入，内部不请求 |
| common | `components/common` | ui、charts、lib、types | 跨业务复用的复合件（卡片、状态、录音） |
| layout | `components/layout` | ui、common、store | 应用骨架，只在 `AppShell` 中使用 |
| feature | `features/*` | 以上全部 + services | 页面与其专用组件，不对外导出给其他 feature |

新增组件的判断顺序：能在 `ui` 改 variant 解决 → 不改；是两个以上页面共用的模式 →
`common`；只服务一个页面 → 放该 feature 的 `components/`。

---

## 2. ui 基础组件

### Button

```tsx
<Button variant="default" size="sm" onClick={fn}>开始</Button>
<Button variant="ghost" size="icon-sm" aria-label="关闭"><X /></Button>
<Button asChild><Link to="/">回到首页</Link></Button>
```

| Variant | 用途 |
|---|---|
| `default` | 页面唯一主操作 |
| `outline` | 次级操作（白底 + 发丝线） |
| `subtle` | 区块内行动，品牌浅底 |
| `ghost` | 图标/行内弱操作 |
| `ember` | 强调性操作（少用） |
| `destructive` | 删除 |
| `link` | 文字按钮 |

尺寸：`sm` 32 / `default` 36 / `lg` 40 / `icon` 36 / `icon-sm` 32。

### Badge

`variant`: `default`(中性底) `brand` `ember` `outline` `success` `warning` `danger` `solid`。
用于状态与分类，字号固定 11px —— 不要用它当标题。

### Tabs

下划线式标签，`TabsList` 自带 `border-b`，`TabsTrigger` 选中态为品牌色下划线。
测评、训练、知识库视图切换使用。

### Dialog / AlertDialog

- `Dialog`：命令面板、需要输入或长内容的浮层。
- `AlertDialog`：破坏性确认。项目内统一用 `ConfirmDialog` 封装，不要直接写 AlertDialog。

### Select / Switch / Slider / Checkbox / Input / Textarea

全部使用发丝线（`shadow-hairline`）+ 品牌色焦点环，尺寸与按钮对齐（32–36px 高）。

---

## 3. common 复合组件

### PageContainer / PageHeader / SectionHeader

```tsx
<PageContainer>
  <PageHeader
    title="学习数据"
    description="一句话说明这个页面能回答什么问题"
    meta={<span className="text-2xs text-ink-muted">辅助信息</span>}
    actions={<Button>主操作</Button>}
  />
  <SectionHeader title="区块标题" description="可选副标题" action={<Button size="sm" variant="ghost">更多</Button>} />
</PageContainer>
```

- `PageHeader.title` 使用衬线 `type-page-title`；全站页面标题只在这里出现一次。
- `SectionHeader` 不加全大写 eyebrow，层级靠字号与字重表达。
- `PageSplit({ children, aside, asideWidth })` 提供「主列 + 侧列」布局，桌面端优先。

### StatCard

```tsx
<StatCard label="平均正确率" value="86" unit="%" delta={3} deltaLabel="%" trend={[62,71,74,80,83,86]} />
<StatCard variant="bare" ... />  // 用于数据带内，无边框
```

### ProgressCard

带分段占比的进度卡，`segments` 会替代默认单色进度条：

```tsx
<ProgressCard title="词汇" value={4} total={18} segments={[{label:"已掌握", value:4, color:"var(--success)"}]} />
```

### TaskCard

Dashboard 时间轴任务行。`index` 提供序号与连接线（仅在真实有序列表时使用）。
`status` 决定序号圆点的样式与 Badge 文案。

### KnowledgeCard / WordCard / GrammarCard

- `KnowledgeCard`：列表项，展示类型、难度、掌握度、到期时间，`active` 表示选中。
- `WordCard`：词条卡片，`compact` 用于紧凑列表；`onSpeak` 触发本地 TTS。
- `GrammarCard`：语法点卡片，含结构公式与常见错误。

### ExerciseCard

训练入口卡。`option.icon` 是字符串键（数据层不存组件），映射在组件内完成——
这样 Mock/AI 数据只携带可序列化的描述。

### ChartCard

图表外壳：标题、指标、右上操作、底部图例/说明。所有图表都应包在 ChartCard 里。

### EmptyState / LoadingState / ErrorState

```tsx
<LoadingState variant="list" rows={4} />
<LoadingState variant="cards" rows={3} />
<LoadingState variant="chart" />
<EmptyState icon={Inbox} title="还没有记录" description="完成第一组训练后这里会出现数据" action={<Button>开始</Button>} />
<ErrorState error={apiError} onRetry={reload} />
```

### ConfirmDialog

```tsx
<ConfirmDialog open={open} onOpenChange={setOpen} title="清理本地记录？" description="这个操作无法撤销。" destructive confirmLabel="清理" onConfirm={fn} />
```

### MasteryBar / VoiceRecorder

- `MasteryBar`：0–100 掌握度条，颜色随区间自动在 success/brand/ember/danger 间切换。
- `VoiceRecorder`：录音控件，权限被拒或环境不支持时降级为文字提示，不阻塞练习。

---

## 4. charts

| 组件 | 关键 props | 说明 |
|---|---|---|
| `Sparkline` | `values, height, fill` | 方向性迷你线，无坐标轴 |
| `LineChart` | `series[{id,label,color,values,dashed}], labels, yMax, unit` | hover 十字线 + 数值提示 |
| `BarChart` | `data[{id,label,value,secondary}], unit` | 柱高严格映射 |
| `Heatmap` | `cells[{date,minutes}], max` | 透明度映射强度 |
| `RadialGauge` | `value, label, benchmark, size` | 可选基准刻度 |
| `RadarChart` | `axes[{label,value,benchmark}], size` | 实线=用户，虚线=基准 |

所有图表都用 `var(--brand)` 等 CSS 变量取色，主题切换无需重绘。

---

## 5. layout

- `AppShell`：侧栏 + 顶栏 + `<Outlet/>` + 命令面板 + 全局快捷键（⌘/Ctrl K）。
- `AppSidebar`：7 个一级导航，折叠时保留 tooltip，底部折叠按钮。
- `AppHeader`：面包屑、全局搜索入口、主题切换、同步徽章、账户菜单。
- `CommandPalette`：页面跳转 + 常用操作，支持 ↑↓ / Enter / Esc。
- `nav-config.ts`：导航单一事实源（图标、路径、中英文名、描述）。

---

## 6. 训练体系的组件契约

这是本项目的核心抽象：**题型数据 → 统一渲染器 → 统一判分 → 统一结果**。

```
types/exercise.ts (可辨识联合 Exercise)
        │
        ├─ services/mock/exercises.ts   题库
        ├─ features/training/components/answer-controls.tsx   每种题型的输入控件
        ├─ features/training/components/exercise-view.tsx     switch(kind) 分发
        ├─ lib/grading.ts                scoreAnswer(exercise, value) → {correct, partial}
        └─ features/training/components/training-result.tsx   统一结果面板
```

运行器：

- `TrainingSessionRunner({ session, onSubmit, onExit })` — 单题提交 → 即时反馈 → 下一题 →
  最后一题交给页面调用 `trainingService.submitSession`。
- `AssessmentRunner({ module, onSubmit, onExit })` — 按分项顺序平铺所有题目，复用
  `ExerciseView` 与 `scoreAnswer`。

### 新增一种题型（完整步骤）

1. `types/exercise.ts`：给 `ExerciseKind` 加值，定义 `XxxExercise extends ExerciseBase`，
   并入 `Exercise` 联合。
2. `services/mock/exercises.ts`：写题目数据，加入 `exerciseBank`；如需训练入口，追加
   `trainingOptions`。
3. `lib/grading.ts`：在 `scoreAnswer` 中加 `case "xxx"`，返回 `{ correct, partial }`。
4. `features/training/components/answer-controls.tsx`：写输入控件（受控，
   `value/onChange/revealed`）。
5. `features/training/components/exercise-view.tsx`：`renderControl` 中加分支。
6. 如果算作独立训练方式，在 `mock/assessment.ts` 里也可以复用同一题目。

无需修改：页面、结果页、状态管理、服务层。

---

## 7. 知识图谱组件

`KnowledgeGraph({ nodes, edges, selectedId, matchedIds, onSelect })`

- 布局：数据层给出归一化初始坐标，组件内做 160 次局部松弛（斥力 + 边弹簧 + 向心）。
- 交互：点击选中、拖拽移动节点、滚轮缩放、空白处平移、点击空白取消选中。
- 视觉：颜色按 `kind`，半径按 `weight`，选中节点显示光环，邻居高亮，非邻居自动降透明。
- 标签：选中/邻居/非单词节点或放大到 115% 时显示，避免默认状态文字拥挤。

详情面板 `KnowledgeDetailPanel` 接收 `KnowledgeDetail` 可辨识联合，`switch(detail.kind)`
拿到精确类型，不使用 `any` 或类型断言。

---

## 8. 编码约定

1. 组件用函数声明 + 具名导出（不用 default export）。
2. Props 用本地 `interface XxxProps`，超过 6 个属性时考虑拆分组件。
3. 颜色、字号、阴影一律走 token 类名；禁止 `style={{ color: "#..." }}`。
4. 内联 `style` 只用于数据驱动的值（宽度百分比、图表坐标、CSS 变量引用）。
5. 图标按钮必须有 `aria-label`；纯装饰图形加 `aria-hidden`。
6. 不给每个卡片加 hover 动效；hover 只表示「可点击」。
7. 文案：主动语态、sentence case、动词开头，CTA 说明结果（「开始跟读」而不是「提交」）。
8. 错误文案说明「出了什么问题」与「怎么修复」，不道歉、不含糊。
