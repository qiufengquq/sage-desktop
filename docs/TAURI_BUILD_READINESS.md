# Tauri 构建环境就绪性报告（第二阶段 · 2.1）

> 目的：确认当前工程**具备构建与运行条件**，并记录已验证的命令、结果与遗留问题。
> 约束：本阶段不修改 UI、不修改业务逻辑、不调整系统权限、不安装额外依赖、不追求发布产物。
> 记录时间：2026-11-14。

---

## 1. 结论

**环境就绪，可以进入下一阶段的开发与联调。**

- 工具链、WebView2、MSVC 全部就位（`npx tauri info` 除 Packages/Plugins 段外全绿）。
- 前端四道关卡全部通过：`typecheck` / `lint` / `build` / `vitest (73)`。
- Rust 侧 `cargo test` 通过（75 + 5 = 80 项），`cargo build` 成功产出 `src-tauri/target/debug/sage.exe`。
- 未执行 `tauri build`（发布打包）与 GUI 启动，原因见 §5，并**不属于本阶段目标**。

---

## 2. 环境清单（实测）

| 项 | 值 | 状态 |
|---|---|---|
| 操作系统 | Windows 10.0.26200 x86_64 | ✔ |
| WebView2 Runtime | 157.0.4341.46 | ✔ 已安装 |
| MSVC | Visual Studio 生成工具 2022 | ✔ |
| rustc | 1.99.1 (58cf8f9c3, 2026-10-13) | ✔ |
| cargo | 1.99.1 (5f94df478, 2026-08-27) | ✔ |
| rustup / toolchain | 1.29.2 / `stable-x86_64-pc-windows-msvc`（default） | ✔ |
| Node | v24.21.0 | ✔ |
| npm | 11.19.0 | ✔ |
| pnpm | 10.33.0（存在，项目用 npm） | 未使用 |
| Tauri CLI | `tauri-cli 2.12.4` | ✔ |
| tauri crate | 2.12.5 | ✔ |
| tauri-build | 2.7.4 | ✔ |
| wry / tao | 0.57.2 / 0.37.2 | ✔ |
| @tauri-apps/api | 2.12.4 | ✔ |

证据：`npx tauri info`。

---

## 3. 已验证的构建命令

### 3.1 前端

| 命令 | 结果 |
|---|---|
| `npm run typecheck`（`tsc -b`） | ✔ 无输出、退出码 0 |
| `npm run lint`（`oxlint src tests`） | ✔ `Found 0 warnings and 0 errors`（148 files / 114 rules） |
| `npm run build`（`tsc -b && vite build`） | ✔ `built in 923ms`，产物落 `dist/`（`index.html` + `assets/*`，最大 chunk 472 kB / gzip 153 kB） |
| `npm test`（`vitest run`） | ✔ 11 files / **73 tests passed**，2.76s |

### 3.2 Rust

| 命令 | 结果 |
|---|---|
| `cargo test`（`src-tauri`） | ✔ `75 passed`（lib）+ `5 passed`（集成）+ 2 个空 target，0 failed |
| `cargo build`（debug） | ✔ `Finished dev profile`，增量 0.34s；产物 `src-tauri/target/debug/sage.exe` 存在 |
| `cargo check`（被 build 覆盖） | ✔ |

### 3.3 Tauri

| 检查 | 结果 |
|---|---|
| `npx tauri info` | ✔ 环境自检通过 |
| `beforeDevCommand` = `npm run dev` | ✔ 脚本存在 |
| `devUrl` = `http://localhost:1420` | ✔ 与 `vite.config.ts` 的 1420 一致（Vite 服务端口） |
| `beforeBuildCommand` = `npm run build` | ✔ 先构建前端再嵌入 |
| `frontendDist` = `../dist` | ✔ 目录已生成，`index.html` 存在 |
| 图标 | ✔ `32x32 / 128x128 / 128x128@2x / icon.ico / icon.icns` 齐全（另含 11 个 Windows Store 尺寸） |
| 产物 | ✔ `target/debug/sage.exe` |

---

## 4. 配置审查

### 4.1 `tauri.conf.json`

| 项 | 值 | 评价 |
|---|---|---|
| `identifier` | `com.sage.learning` | ✔ 稳定，勿轻易改（影响数据目录） |
| `frontendDist` | `../dist` | ✔ |
| `build.devUrl` | `http://localhost:1420` | ✔ 与 Vite 一致 |
| 窗口 | 1440×920，min 1080×700，居中、可缩放、无边框透明 | ✔ 桌面定位合理；最小宽度 1080 对小屏不友好（非阻塞） |
| CSP | `default-src 'self'`；`img-src` 含 `asset:`/`data:`/`blob:`；`style-src 'unsafe-inline'`；`connect-src` 含 `ipc:` | ✔ 收紧得当；`style-src 'unsafe-inline'` 为 Tailwind 与 Radix 所需 |
| `bundle.targets` | `"all"` | ⚠ Windows 上会产出 **MSI(WiX) + NSIS**，两者首次构建需联网下载工具链，见 §5 |

### 4.2 `capabilities/default.json`

```json
{ "identifier": "default", "windows": ["main"], "permissions": ["core:default"] }
```

- ✔ **最小权限**：只有 `core:default`，没有 `fs`、`shell`、`http`、`dialog`、`updater`。
- ✔ 与实现一致：所有文件/数据库操作都在 Rust 侧完成，前端只走 `invoke`。
- ⚠ 缺少 `tauri-plugin-single-instance`（计划阶段 D 接入，防止双开写同一 SQLite）。

### 4.3 `Cargo.toml`

| 项 | 值 | 评价 |
|---|---|---|
| `rust-version` | `1.77` | ⚠ 实际工具链 1.99.1，未验证 1.77 能否编译（rusqlite 0.32 依赖较新语法）；建议改为 `1.80` 或直接删除该字段 |
| `rusqlite` | `0.32` + `bundled` + `backup` | ✔ 无系统依赖，备份 API 已用 |
| `chrono` / `sha2` | 0.4 / 0.10 | ✔ 用途明确（时间格式化、迁移校验和） |
| `[profile.release]` | `lto=true, codegen-units=1, opt-level="s", strip=true, panic="abort"` | ✔ 体积优先；代价是 release 构建慢（预计 3–8 分钟），见 §5 |

---

## 5. 未验证项与原因

| 项 | 状态 | 原因 | 建议 |
|---|---|---|---|
| `npm run tauri:dev`（真实窗口启动） | **未执行** | 需要交互式 GUI 会话并长期占用进程；本阶段要求"记录结果"，不以启动成败判定就绪 | 由你本地执行一次确认首屏与 `app_info` 返回 |
| `npm run tauri build`（发布包） | **未执行** | ① 明确不属于本阶段目标；② `targets:"all"` 需联网下载 WiX/NSIS；③ release LTO+strip 耗时长且会写大量产物 | 进入 2.6 验收前执行一次 `tauri build --no-bundle` 验证链接，再按需打包 |
| MSI / NSIS 打包 | 未验证 | 同上 | 若不发布，可把 `bundle.targets` 改为 `["nsis"]` 或 `["msi"]` 减少依赖 |
| 签名 / 公证 | 不涉及 | 桌面分发策略未定 | 后续单独排期 |
| 双开写库 | 未验证 | 未接 `single-instance` | 阶段 2.4 接入插件，或依赖 WAL + `busy_timeout=5s` |
| 低版本 Windows（Win10 1809 以下） | 未验证 | WebView2 最低要求 | 目标用户默认 Win10 2004+ 即可 |

---

## 6. 就绪性风险登记

| # | 风险 | 影响 | 处置 |
|---|---|---|---|
| T1 | `bundle.targets: "all"` 需联网下载打包器 | 离线/CI 构建失败 | 首次打包前预热，或改为单一 target |
| T2 | release 启用 `lto+strip+panic=abort` | 构建 3–8 分钟；`panic=abort` 使部分 panic 无法 unwind | 保留（桌面端合理），CI 缓存 `target/` |
| T3 | `rust-version = 1.77` 与实际依赖不符 | 未来被 `cargo` 严格校验时报错 | 提升到 1.80+ 或移除 |
| T4 | 数据库放在 OneDrive 同步目录 | WAL 文件被同步 → 损坏风险 | `state.rs:27-36` 已告警；文档需强调"不要放 OneDrive" |
| T5 | `devUrl` 端口 1420 被占用 | `tauri dev` 起不来 | `vite.config.ts` 设 `strictPort` 或换端口 |
| T6 | 无单实例保护 | 两个实例同时写库 | 2.4 接 `tauri-plugin-single-instance` |
| T7 | 打包态未实现命令直接报 `unavailable` | 首页/分析/规划/复习空白 | 这是**功能缺口**而非构建缺口，见 `BUSINESS_MODEL_REVIEW.md` C1–C5，2.3–2.5 修复 |

---

## 7. 复核命令（可复现）

```powershell
# 前端
cd d:/文件/project/sage
npm run typecheck ; npm run lint ; npm run build ; npm test

# Rust
cd src-tauri
cargo test ; cargo build

# 环境与配置
npx tauri info

# 仅验证链接（不做发布，推荐在 2.6 前跑一次）
npx tauri build --no-bundle
```

---

## 8. 本阶段对工程的改动

**无。** 本次仅执行了只读性质的构建/测试命令，未修改 `tauri.conf.json`、`capabilities/`、`.env`、系统权限或任何源码。`dist/` 与 `target/` 属构建产物，未提交。
