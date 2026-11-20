# Sage Design System — Instrument

> 一句话：把这款软件做成一台**学习仪器**，而不是一个后台管理面板。

本文档是视觉层的单一事实源。代码里的实现位置是 `src/styles/globals.css`（设计变量）
与 `src/components/ui/*`（基础组件）。任何页面级样式都应引用这里的 token，不允许出现
硬编码的色值、字号与阴影。

---

## 1. 设计立场

| 维度 | 选择 | 理由 |
|---|---|---|
| 气质 | 沉静、精密、略带编辑感 | 学习是长时间专注行为，界面不能抢注意力 |
| 信息密度 | 中等偏高，靠留白与发丝线分隔 | 桌面大屏不需要移动端那种大卡片堆叠 |
| 视觉记忆点 | 只有一个：Dashboard 的编号时间轴 + 铜色关键数据 | 遵循「把大胆集中在一处」 |
| 游戏化 | 只体现在反馈层（对错投影、连击、进度轨道） | 不做卡通插画与夸张弹跳，避免与学习工具气质冲突 |
| 知识管理 | 双链 + 局部图谱的组织方式 | 借鉴 Obsidian 的信息结构，不借鉴它的配色 |

### 明确禁止（AI 模板化特征清单）

1. 紫粉渐变、玻璃拟态、彩色光晕背景。
2. 暖奶油底 `#F4F1EA` + 陶土色 `#D97757` 这类「AI 默认色组」。
3. 每张卡片同一圆角、同一柔和灰影、内容切块等分的「卡片汤」。
4. 每个区块都 fade-and-slide-up 入场；每张卡片都有 hover transition。
5. 标题上方通用全大写 eyebrow、`A · B · C` 中点元信息、正文里滥用 `→`。
6. 只用于装饰的编号、分隔线、标签——结构装置必须承载信息。

---

## 2. 色彩

色彩按**语义角色**定义，浅色与深色共用同一组 token 名，切换主题只换变量值。

| Token | 浅色 | 深色 | 用途 |
|---|---|---|---|
| `--paper` | `#F7F7F4` | `#0F1211` | 页面底色 |
| `--surface` | `#FFFFFF` | `#161A18` | 卡片、面板、输入框 |
| `--raised` | `#FCFCFA` | `#1B201E` | 次级容器、表头 |
| `--sunken` | `#F0F1EC` | `#0B0E0D` | 内嵌区域、轨道、hover 底 |
| `--ink` | `#14171A` | `#E9EBE6` | 主文字 |
| `--ink-soft` | `#3D4442` | `#C3C9C2` | 正文、次要信息 |
| `--ink-muted` | `#6B7370` | `#8B938F` | 辅助文字、标签 |
| `--line` | `#E2E4DF` | `#262C29` | 发丝线、边框 |
| `--line-strong` | `#CDD1CA` | `#37403C` | 强调边框、滚动条 |
| `--brand`（墨绿） | `#1F5E4A` | `#5AAB8D` | 主色：选中态、主按钮、焦点环 |
| `--brand-tint` | `#E7EFE9` | `#17281F` | 主色浅底 |
| `--ember`（铜） | `#A8622B` | `#D99A5B` | **限量**强调：连续学习、经验值、警示级提醒 |
| `--success` | `#2F7A57` | `#5FB08A` | 正确、已掌握 |
| `--warning` | `#A26D17` | `#D9A45B` | 部分正确、待补强 |
| `--danger` | `#A63A32` | `#D9736B` | 错误、逾期、破坏性操作 |
| `--info` | `#2B5F7E` | `#6FA5C0` | 第三序列（图表） |

规则：

- 一个视图中**品牌色只承担「当前／主操作」语义**，其余用中性层级表达。
- 铜色在单个区块内最多出现一次。它是强调，不是第二种主题色。
- 图表只使用 `--chart-1..5`（= brand / ember / info / success / muted），不做彩虹配色。
- 正文对比度按 WCAG AA（≥ 4.5:1）设计；`--ink-muted` 仅用于辅助信息。

---

## 3. 字体

| 角色 | 字体 | 用途 |
|---|---|---|
| 显示 / 标题 | **Newsreader**（可变衬线） | 页面标题、区块标题、单词原形、例句 |
| UI / 正文 | **Instrument Sans**（可变无衬线） | 导航、按钮、表单、说明文字 |
| 数据 / 音标 / 代码 | **JetBrains Mono** | 数值、时间、音标、结构公式、键位提示 |

中文字体回退：`PingFang SC → Microsoft YaHei → Noto Sans SC`。
字体全部通过 `@fontsource-variable/*` 本地打包，桌面端离线可用，不依赖网络。

### 字阶

| Token | 字号 / 行高 | 用途 |
|---|---|---|
| `text-2xs` | 11 / 16 | 标签、元信息、表格辅助列 |
| 正文默认 | 15 / 1.6 | 段落 |
| `text-[13px]` ~ `text-[13.5px]` | 13–13.5 | 控件、列表主文本 |
| `type-section-title` | 22 / 1.3 | 区块标题（衬线） |
| `type-page-title` | 44 / 1.08，字距 -0.022em | 页面主标题（衬线） |

正文行宽控制在 60–70 字符（`max-w-[62ch]`）；标题使用 `text-wrap: balance`，正文使用
`text-wrap: pretty`。

---

## 4. 间距

基础单位 `0.25rem`（Tailwind 默认）。项目约定：

| 场景 | 值 |
|---|---|
| 页面内边距 | `px-6 py-6` → `lg:px-9 lg:py-7` |
| 区块之间 | `space-y-7`（28px） |
| 卡片内边距 | `p-4`（16px）；密集列表用 `px-4 py-2.5` |
| 控件间距 | `gap-2` / `gap-2.5` |
| 图标与文字 | `gap-1.5` |

内容最大宽度 `max-w-[1240px]`，居中。桌面端优先，最小支持宽度 1024px。

---

## 5. 圆角

分级使用，禁止全场同一半径：

| Token | 值 | 用于 |
|---|---|---|
| `--radius-xs/sm` | 4 / 6px | 徽章、小标签、内嵌块 |
| `--radius-md` | 8px | 卡片、输入框、按钮、面板 |
| `--radius-lg` | 12px | 对话框、抽屉 |
| `9999px` | 全圆 | 开关、圆形图标按钮、进度轨道 |

---

## 6. 阴影

发丝线是主力分隔手段，阴影只用于**浮层**，不给普通卡片加投影。

| Token | 用途 |
|---|---|
| `--shadow-hairline` | `inset 0 0 0 1px var(--line)`，卡片/输入框/次级按钮 |
| `--shadow-card` | 极浅投影，仅用于主按钮与主卡片 |
| `--shadow-lift` | hover 抬升、tooltip |
| `--shadow-pop` | 下拉菜单、对话框、详情浮层 |

---

## 7. 按钮

| 变体 | 外观 | 用途 |
|---|---|---|
| `default` | 墨绿实底 | 页面唯一主操作 |
| `outline` | 白底 + 发丝线 | 次级操作 |
| `subtle` | 墨绿浅底 | 区块内联行动（如"去训练"） |
| `ghost` | 无底，hover 变浅 | 图标按钮、行内操作 |
| `ember` | 铜色实底 | 极少数强调性操作 |
| `destructive` | 红底 | 删除/清理 |
| `link` | 文字按钮 | 弱操作 |

尺寸：`sm` 32px、`default` 36px、`lg` 40px、`icon` 36×36。
所有按钮：`focus-visible` 显示 2px 品牌色焦点环；`active` 时向下位移 1px，模拟实体按键。

---

## 8. 卡片与容器

三种形态，按信息量选择，不要全部做成卡片：

1. **Panel（主力）**：`rounded-md border border-line bg-surface`，用于成组的信息。
2. **Banded strip（数据带）**：单个面板 + `divide-x divide-line` 分成 3–4 个指标格，
   Dashboard 顶部与设置页使用，避免四张并排卡片。
3. **Bare section（无框区块）**：只有标题 + 内容，靠 `border-t` 或间距分隔，用于长文与建议列表。

列表行统一：`px-4 py-2.5`，hover `bg-raised`，分隔用 `divide-y divide-line`。

---

## 9. 导航

- 左侧固定导航 248px，折叠后 68px；折叠状态用 tooltip 保留可发现性。
- 选中态：`bg-surface` + 发丝线 + 左侧 2px 品牌色竖条 + 图标加粗，不做整块色块。
- 顶部工具栏 56px：面包屑（Sage › 当前模块）、全局搜索（⌘/Ctrl K）、主题切换、同步状态、账户菜单。
- 页面标题在内容区（衬线大字），不在工具栏里重复。

---

## 10. 图表规范

全部为手绘 SVG（`src/components/charts/*`），不引入图表库，原因：主题 token 直连、
视觉与界面一致、包体更小。

| 组件 | 映射规则 |
|---|---|
| `Sparkline` | 仅表达方向；无坐标轴 |
| `LineChart` | 多序列共享 y 轴，hover 显示十字线与数值；网格线用 `--line` 虚线 |
| `BarChart` | 柱高严格 = value / max，不做视觉夸张 |
| `Heatmap` | 透明度 = 分钟数 / 区间最大值；0 分钟为 6% 不透明 |
| `RadialGauge` | 弧长 = 百分比；可选 benchmark 刻度线表示同水平均值 |
| `RadarChart` | 实心多边形 = 用户分数，虚线多边形 = 同水平基准 |

禁止：为视觉冲击而截断坐标轴、编造不存在的数据点、用面积图夸大差异。

---

## 11. 动效

| 场景 | 时长 / 曲线 |
|---|---|
| hover / 颜色变化 | 150ms `ease-instrument` |
| 展开、折叠、宽度变化 | 200ms |
| 进度条与弧线 | 500–700ms |
| 浮层与对话框 | 240–320ms，`animate-fade` / `animate-rise` |

`--ease-instrument: cubic-bezier(0.22, 0.61, 0.36, 1)`。
全局遵守 `prefers-reduced-motion: reduce`，此时所有动画缩短至 0.001ms。

动效只允许出现在**用户动作的响应**上（提交、切换、展开、拖拽）。页面进入不做逐块淡入。

---

## 12. 无障碍

- 所有交互元素可 Tab 到达，`:focus-visible` 有 2px 品牌色轮廓（全局定义）。
- 图标按钮必须带 `aria-label`；纯装饰图标 `aria-hidden`。
- 开关、筛选、视图切换使用 `aria-pressed` / `role="radiogroup"` 等语义。
- 页面提供「跳到主内容」的 skip link。
- 颜色不是唯一信息载体：正确/错误同时用图标与文案表达。
- 输入框与 `Label` 关联，错误状态用 `aria-invalid`。

---

## 13. Token → Tailwind 映射速查

| CSS 变量 | Tailwind 类 |
|---|---|
| `--brand` | `bg-brand` `text-brand` `border-brand` `ring-brand` |
| `--ember` | `bg-ember` `text-ember` `bg-ember-tint` |
| `--ink` / `--ink-soft` / `--ink-muted` | `text-ink` / `text-ink-soft` / `text-ink-muted` |
| `--line` | `border-line` `divide-line` `bg-line` |
| `--paper` / `--surface` / `--raised` / `--sunken` | `bg-paper` / `bg-surface` / `bg-raised` / `bg-sunken` |
| `--shadow-hairline` 等 | `shadow-hairline` `shadow-card` `shadow-lift` `shadow-pop` |
| `--font-display` | `font-display`（或 `type-page-title` 等语义类） |

排版语义类：`type-page-title`、`type-section-title`、`type-label`、`type-numeral`。
