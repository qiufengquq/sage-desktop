# 图表渲染规范与实现记录

本文档描述 `components/charts` 下 SVG 图表的渲染不变量、`LineChart` 的完整契约、接入方式，
以及本次交付的变更清单与待办。

相关文档：[组件指南](./COMPONENT_GUIDE.md) · [设计系统](./DESIGN_SYSTEM.md)

---

## 1. 核心不变量：坐标与像素 1:1

### 问题

手绘 SVG 图表若使用固定宽度的 `viewBox`（例如 `viewBox="0 0 640 200"`），同时让 `<svg>` 跟随
容器自适应宽度，浏览器会按 `preserveAspectRatio` 的默认值 `xMidYMid meet` 做**等比缩放**。
当容器宽度不等于 640 时，整张图（含文字）被横向拉伸或压缩：

- 容器变窄 → 文字被压扁、笔画变细、10px 刻度实际渲染成 7–8px，几乎不可读；
- 容器变宽 → 文字被横向拉开，字形失真。

### 解法

**让 `viewBox` 的宽度恒等于容器的实测像素宽度。** 这样 SVG 坐标系中的一个单位就等于一个 CSS 像素，
缩放比为 1:1，文字与线宽都按声明的像素值精确渲染。

```tsx
const { ref, width } = useElementWidth<HTMLDivElement>();

<div ref={ref} className="relative w-full">
  <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`}>
```

三条必须同时满足的条件：

1. `viewBox` 的 `width`/`height` 与 `<svg>` 的 `width`/`height` 使用同一组数值；
2. 宽度来自 `ResizeObserver` 实测，而非 `window.innerWidth` 之类的估算；
3. 字号、线宽写成固定值（`fontSize: 10`、`strokeWidth={1.75}`），不参与任何比例换算。

> 违反任意一条，文字失真问题就会复现。新增图表组件时请沿用这条约束。

---

## 2. `useElementWidth`

位置：`src/hooks/use-element-width.ts`

```ts
export function useElementWidth<T extends HTMLElement>(fallback = 640): {
  ref: React.RefObject<T | null>;
  width: number;
}
```

用于观测容器宽度，供手绘 SVG 图表使用。用实测像素作为 `viewBox` 宽度，可保证 1:1 映射，
文字不会被拉伸或缩得过小。

| 项 | 说明 |
|---|---|
| 默认回退值 | `640`，仅在挂载瞬间无法测量时使用 |
| 下限钳制 | `Math.max(260, …)`，避免容器塌陷到 0 造成除零或负值布局 |
| 取整 | `Math.round`，避免 `viewBox` 出现小数导致子像素模糊 |
| 双通道测量 | `ResizeObserver` 处理后续变化；挂载时立刻 `getBoundingClientRect()` 补首帧 |
| 异常防护 | `Number.isFinite(next)` 过滤 `NaN` |

使用方式：

```tsx
const { ref, width } = useElementWidth<HTMLDivElement>();
return <div ref={ref}>…</div>;
```

---

## 3. `LineChart`

位置：`src/components/charts/line-chart.tsx`

手绘 SVG 折线图。`viewBox` 宽度取容器实测宽度，坐标与像素 1:1，文字不会被非等比缩放拉变形。

### 3.1 Props

```ts
export interface LineSeries {
  id: string;
  label: string;
  color: string;
  values: number[];
  dashed?: boolean;
}

interface LineChartProps {
  series: LineSeries[];
  labels: string[];
  height?: number;
  yMax?: number;
  yMin?: number;
  unit?: string;
  className?: string;
}
```

| Prop | 默认值 | 说明 |
|---|---|---|
| `series` | 必填 | 数据序列；`color` 传 CSS 变量（如 `var(--brand)`），不要传十六进制 |
| `labels` | 必填 | x 轴标签，长度应与各序列 `values` 一致 |
| `height` | `200` | 图表区高度（含上下内边距） |
| `yMax` | 自动 | 缺省时取 `Math.ceil((dataMax * 1.12) / 10) * 10`，留 12% 顶部余量并取整到十位 |
| `yMin` | `0` | y 轴下限 |
| `unit` | `""` | 追加在 tooltip 数值后的单位，如 `" 分钟"`、`"%"` |
| `className` | – | 合并到最外层容器 |

### 3.2 布局常量

| 常量 | 值 | 作用 |
|---|---|---|
| `padLeft` | 34 | 容纳 y 轴刻度文字 |
| `padRight` | 10 | 右侧留白 |
| `padTop` | 12 | 顶部留白 |
| `padBottom` | 24 | 容纳 x 轴标签 |
| `gridLines` | 4 | 横向网格条数（实际绘制 5 条，含底线） |

```ts
const plotW = Math.max(10, width - padLeft - padRight);
const plotH = Math.max(10, height - padTop - padBottom);
```

`plotW`/`plotH` 各设 10px 下限，防止极端窄容器下出现零宽绘图区。

### 3.3 坐标映射

```ts
const xAt = (i: number) => padLeft + (count <= 1 ? plotW / 2 : (i / (count - 1)) * plotW);
const yAt = (value: number) => padTop + plotH - ((value - yMin) / (max - yMin || 1)) * plotH;
```

- `count <= 1` 时 `xAt` 返回绘图区中心，规避 `count - 1 = 0` 的除零。
- `yAt` 中的 `|| 1` 防止 `yMin === yMax` 时除零。

### 3.4 已实现能力

| 能力 | 实现要点 |
|---|---|
| 多序列折线 | `series.map` 逐条生成 `path`，`strokeWidth={1.75}`、圆头圆角连接 |
| 虚线序列 | `strokeDasharray={s.dashed ? "4 3" : undefined}` |
| 网格与 y 轴刻度 | 5 条横线，倒序第一条实线、其余 `"2 5"` 虚线；刻度 `fontSize: 10` + `var(--font-mono)`，`textAnchor="end"` |
| x 轴标签自适应抽样 | `labelStep = Math.max(1, Math.ceil(count / Math.max(4, Math.floor(width / 78))))`，按每 78px 约一个标签估算容量 |
| 悬停索引反算 | `Math.round(((x - padLeft) / plotW) * (count - 1))` 后钳制到 `[0, count - 1]` |
| 悬停视觉 | 垂直参考线（`var(--line-strong)`）+ 每条序列一个圆点（`r=2.8`，填充 `var(--surface)`、描边取序列色） |
| tooltip | 绝对定位，`left` 为百分比并钳制在 `12%–88%`，`translateX(-50%)` 居中，`bg-surface` + `shadow-pop` |
| 图例 | 仅 `series.length > 1` 时渲染，色块为 `h-0.5 w-4` |
| 无障碍 | `role="img"` + `aria-label="趋势折线图"`；装饰性色块加 `aria-hidden` |

### 3.5 样式约定

只使用既有 token 与工具类，不引入新的颜色/字号字面量：

`text-2xs`、`text-[11px]`、`type-numeral`、`min-w-28`、`rounded-sm`、`shadow-pop`、
`size-1.5`、`h-0.5 w-4`、`bg-surface`、`text-ink`、`text-ink-muted`，以及
`var(--brand)` / `var(--ember)` / `var(--info)` / `var(--line)` / `var(--line-strong)` / `var(--font-mono)`。

> 图表必须包在 `ChartCard` 内使用（见组件指南 §3）。

---

## 4. 接入示例

`src/features/analytics/analytics-page.tsx` 中三处实际用法，覆盖了三种典型形态。

### 4.1 单序列：每日学习量

```tsx
<LineChart
  labels={data.daily.map((d) => formatMonthDay(d.date))}
  series={[
    {
      id: "minutes",
      label: "分钟",
      color: "var(--brand)",
      values: data.daily.map((d) => d.minutes),
    },
  ]}
  height={190}
  unit=" 分钟"
/>
```

### 4.2 多序列：掌握趋势（触发图例）

```tsx
<LineChart
  labels={labels}
  series={data.trends.map((trend, index) => ({
    id: trend.kind,
    label: trend.label,
    color: index === 0 ? "var(--brand)" : index === 1 ? "var(--ember)" : "var(--info)",
    values: trend.series.map((p) => p.value),
  }))}
  height={200}
  yMax={100}
  unit="%"
/>
```

### 4.3 双序列 + 虚线：遗忘曲线

```tsx
<LineChart
  labels={data.forgettingCurve.map((p) => (p.hours < 24 ? `${p.hours}h` : `${Math.round(p.hours / 24)}d`))}
  series={[
    {
      id: "expected",
      label: "理论留存",
      color: "var(--ink-muted)",
      values: data.forgettingCurve.map((p) => p.expected),
      dashed: true,
    },
    {
      id: "actual",
      label: "你的留存",
      color: "var(--brand)",
      values: data.forgettingCurve.map((p) => p.actual),
    },
  ]}
  height={200}
  yMax={100}
  unit="%"
/>
```

> `dashboard-page.tsx` 中的 `LineChart` 是 `lucide-react` 的图标组件，与本节无关。

---

## 5. 新增图表组件的检查清单

1. 用 `useElementWidth` 取宽度，`viewBox` 与 `<svg>` 的宽高使用同一组数值。
2. 字号与线宽写固定值，不做比例换算。
3. 绘图区宽高各设最小值，防止窄容器下除零或负值。
4. 单点 / 空数据 / `min === max` 三种退化情况都要有兜底。
5. 颜色取 CSS 变量，禁止十六进制字面量。
6. `role="img"` + 有意义的 `aria-label`；装饰图形 `aria-hidden`。
7. 用 `ChartCard` 包裹，标题与指标交给外壳，组件本身只管绘图。

---

## 6. 本次变更清单

| 文件 | 类型 | 内容 |
|---|---|---|
| `src/hooks/use-element-width.ts` | 新增 | 容器宽度观测 hook（`ResizeObserver` + 首帧测量 + 下限钳制） |
| `src/components/charts/line-chart.tsx` | 新增 | 手绘 SVG 折线图，含网格、刻度、多序列、虚线、悬停十字线与 tooltip、图例 |
| `src/features/analytics/analytics-page.tsx` | 接入 | 三处调用：每日学习量、掌握趋势、遗忘曲线 |
| `src/components/layout/app-header.tsx` | 样式 | 背景 `bg-raised/60` → `bg-paper`，补齐 `h-(--header-h) shrink-0 items-center gap-3 border-b border-line px-4` |

附带调整：页头高度改用 `h-(--header-h)` 统一声明，与侧栏品牌区保持一致，避免两处高度各自硬编码。

---

## 7. 已知限制与待办

- **未验证**：尚未运行 `tsc` 类型检查、生产构建与视觉回归，窄屏（< 320px）与超长标签场景未实测。
- **交互覆盖**：仅鼠标悬停，缺少键盘导航（方向键移动焦点索引）与触摸长按支持。
- **未迁移**：`bar-chart.tsx`、`heatmap.tsx`、`radar-chart.tsx`、`radial-gauge.tsx`、
  `sparkline.tsx` 为既有实现，尚未统一到本节 §1 的 1:1 映射方案，文字失真风险仍在。
- **未支持**：面积图、堆叠、双 y 轴、数据点标注、导出图片。
