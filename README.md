# Sage

英语智能学习平台的桌面端应用。**1.0.1（Desktop）**：完整桌面端前端与设计系统、
七大业务模块、Rust + SQLite 本地后端，并集成 **SAGE Intelligence**（Library 模式，
源码已内联于本仓库）提供 AI Provider 配置、一次性/流式请求与取消能力。

## 技术栈

React 19 · TypeScript 7（strict）· Vite 8 · Tailwind CSS 4 · shadcn/ui（Radix + CVA）·
Lucide React · Zustand · React Router 8 · Tauri 2 · Rust + rusqlite ·
SAGE Intelligence 0.1.0（`crates/ai-core` / `crates/ai-core-tauri`，本仓库内维护）

## 环境要求

| 依赖 | 版本 | 用途 |
|---|---|---|
| Node.js | ≥ 20（开发使用 24） | 前端构建 |
| Rust | stable ≥ 1.85（开发使用 1.99） | Tauri + SAGE Intelligence |
| WebView2 | Windows 10/11 自带 | 桌面渲染 |

## 快速开始

```bash
# 1. 安装依赖
npm install

# 2. 浏览器内开发（最快，推荐日常迭代）
npm run dev          # http://localhost:1420

# 3. 桌面端开发（带原生窗口，第一次会编译 Rust，约 1–3 分钟）
npm run tauri:dev

# 4. 构建
npm run build        # 前端产物 → dist/
npm run tauri:build  # 桌面安装包 → src-tauri/target/release/bundle/
```

## 脚本

| 命令 | 说明 |
|---|---|
| `npm run dev` | Vite 开发服务器（端口 1420，固定） |
| `npm run build` | 类型检查（`tsc -b`）+ 生产构建 |
| `npm run typecheck` | 仅类型检查 |
| `npm run preview` | 预览生产构建 |
| `npm run tauri:dev` | Tauri 桌面开发模式 |
| `npm run tauri:build` | 打包桌面应用 |

## 数据源

默认使用本地模拟数据。`src/services/api/transport.ts` 依据环境变量分发：

```bash
# .env.local
VITE_SAGE_DATA_SOURCE=mock      # 默认
# VITE_SAGE_DATA_SOURCE=backend # 走 Tauri Command（第二阶段逐步实现）
```

页面不感知数据来源；Mock 与 Rust 返回同一套 `ApiResult<T>`。

## 七大模块

| 路由 | 模块 | 核心内容 |
|---|---|---|
| `/` | 首页 | 编号时间轴任务、今日指标带、掌握度、复习队列、AI 建议、快速开始 |
| `/assessment` | 测评 | 词汇 / 语法 / 口语三个测试，15 个考察维度，含分项结果与能力分级 |
| `/planner` | AI 规划 | 能力雷达画像、今日计划（可勾选）、每周目标、三条带依据的建议、难度滑杆 |
| `/analytics` | 学习数据 | 7/14/30 天聚合、掌握趋势、遗忘曲线、时间分布、待复习/已掌握 |
| `/knowledge` | 知识库 | SVG 力导向图谱（拖拽/缩放/选中）、三轴筛选、详情抽屉、学习历史 |
| `/training` | 训练 | 12 种训练方式，独立输入控件 + 统一判分与结果接口 |
| `/settings` | 设置 | 资料、节奏、提醒、主题、同步状态、本地数据、关于 |

## 目录结构

```
src/
├─ app/          应用装配：providers / router / store
├─ components/   ui（shadcn）· layout · common · charts
├─ features/     dashboard / assessment / planner / analytics / knowledge / training / settings
├─ services/     api（唯一数据入口）· mock（模拟数据与派生计算）
├─ types/        领域类型（可辨识联合优先）
├─ hooks/        useQuery / useThemeSync / useHotkey
├─ lib/          utils / date / format / grading / speech
└─ styles/       globals.css（设计 token 唯一来源）

src-tauri/
├─ src/commands/ Rust Command 骨架（app / user / learning / knowledge）
├─ src/error.rs  AppError → "code:message"
├─ src/state.rs  内存 KV（第二阶段换 SQLite）
└─ capabilities/ 最小权限
```

## 文档

| 文档 | 内容 |
|---|---|
| [docs/DESIGN_SYSTEM.md](docs/DESIGN_SYSTEM.md) | 色彩、字体、间距、圆角、阴影、按钮、图表、动效与反模式 |
| [docs/FRONTEND_ARCHITECTURE.md](docs/FRONTEND_ARCHITECTURE.md) | 分层、数据流、状态管理、路由、错误策略、与 Rust 的边界 |
| [docs/COMPONENT_GUIDE.md](docs/COMPONENT_GUIDE.md) | 每个公共组件的 API 与用法，新增题型的完整步骤 |
| [docs/API_CONTRACTS.md](docs/API_CONTRACTS.md) | 24 条命令契约、错误序列化、AI 结构化数据约定 |

## 设计语言速览

- 名称：**Instrument**（把软件做成一台学习仪器）
- 主色：墨绿 `#1F5E4A`；强调：铜 `#A8622B`（限量）；底：纸白 `#F7F7F4` / 墨黑 `#0F1211`
- 字体：Newsreader（标题）· Instrument Sans（UI）· JetBrains Mono（数据）
- 容器：发丝线为主，阴影只给浮层；数据带替代并排卡片

## 已知限制（第一阶段范围）

- 无真实后端：除 `app_*` / `settings_*` 外的命令返回 `unavailable`，属于预期行为。
- 语音：示范朗读用浏览器 TTS，录音用 `MediaRecorder`，转写用文本输入模拟，未接 ASR。
- 同步：仅有界面与模拟状态。
- 打包：已生成图标与配置，未做代码签名与自动更新。
