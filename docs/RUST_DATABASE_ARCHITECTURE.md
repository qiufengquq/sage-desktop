# Rust 数据访问层架构（第二阶段 · 阶段 A）

> 目标：把 SQLite 接入 Tauri 2 后端，做到"连接统一、事务明确、UI 不阻塞、错误可解释、迁移可回滚"。
> 本文包含 rusqlite 与 SQLx 的结论性比较、模块结构、连接与线程模型、迁移、事务、备份、测试方案。

---

## 1. 驱动选型：rusqlite vs SQLx

### 1.1 比较

| 维度 | rusqlite | SQLx | 对 Sage 的影响 |
|---|---|---|---|
| 执行模型 | 同步（阻塞） | 异步（`sqlx::SqlitePool`） | 命令层本来就是 async，同步驱动可用 `spawn_blocking` 安置；异步驱动省一次线程切换，但收益是**微秒级** |
| SQLite 打包 | `bundled` 特性从源码编译 SQLite | 自带 `libsqlite3-sys` 链接 | 两者都能做到无系统依赖；rusqlite 的 `bundled` 在 Windows 交叉编译时最省心 |
| 编译期校验 | 无（运行时报错） | `query!` 宏需要 `DATABASE_URL` 或 `.sqlx` 离线缓存 | 这是 SQLx 唯一显著优势，但代价见 1.2 |
| 连接池 | 需自配 `r2d2` / `deadpool` | 内置 | Sage 是单写者、数据量极小，池不是必需品（见 §3） |
| 事务 | `Connection::transaction()` → `Transaction`（`&mut Connection`） | `pool.begin()` → `Transaction` | 等价 |
| 迁移 | 自实现 runner（约 100 行）或 `rusqlite_migration` | `sqlx::migrate!` 内置 checksum | SQLx 更省事；rusqlite 自实现才能插入"迁移前强制备份"这一步 |
| 依赖体量 | 小（`rusqlite` + `libsqlite3-sys`） | 较大（`sqlx-core` + `sqlx-macros` + 驱动） | 桌面端首包体积敏感 |
| 出错位置 | 运行期，`rusqlite::Error` 可映射 | 编译期 SQL 语法错 | 数据量小 + 测试覆盖，运行期风险可控 |

### 1.2 为什么**不**选 SQLx

SQLx 的核心卖点是编译期校验 SQL，但它要求构建时能连一个真实数据库（`DATABASE_URL`）或维护 `.sqlx` 离线缓存目录。对本项目的具体成本：

1. **构建环境变复杂**：`npm run tauri:build` 之前需要 `sqlx-cli prepare` 或一个空库，CI 与本地都得配。当前 `src-tauri` 的构建链是纯 `cargo build`（`Cargo.toml:13-23`），没有额外前置步骤。
2. **`payload_json` 场景拿不到收益**：11 种题型塞在 JSON 里（`DATABASE_DESIGN.md` §2.2），最复杂的那部分 SQL 恰恰无法被宏校验。
3. **异步没有真实收益**：本地文件库单条查询在毫秒以下，瓶颈从来不是"等待 IO"，而是"占用主线程"。这个问题 `spawn_blocking` 一样解决（§2.2）。
4. **迁移控制权**：迁移失败要"先备份、再执行、失败回滚并保留旧文件"，自实现 runner 才能把备份串进去。

> **结论：采用 `rusqlite`（`bundled` + `backup` 特性）**。SQLx 不是错的，只是对这个项目的成本/收益比更差。若未来出现"远端数据库同步"需求（不属于本项目范围），再评估迁移成本 —— 仓储层已用 trait 隔离（§4.3），替换面可控。

### 1.3 依赖清单

```toml
# src-tauri/Cargo.toml
[dependencies]
rusqlite = { version = "0.32", features = ["bundled", "backup"] }
chrono   = { version = "0.4", features = ["serde"] }
sha2     = "0.10"     # 迁移脚本校验和；不想加依赖可换成 std DefaultHasher
```

`bundled` 让 SQLite 随二进制编译，用户机器不需要安装任何东西。`backup` 启用 `Connection::backup()`（在线备份 API，比复制文件安全）。

`chrono` 只用于时间格式化与解析；**所有时间以 ISO8601 UTC 文本存储**，不引入 `time` crate。若想彻底不依赖 `chrono`，可用 `std::time` + 手写格式化，但时区（`profile.timezone`）与日切计算会明显变难，不建议。

---

## 2. 线程模型与"不阻塞 UI"

### 2.1 Tauri 2 的命令执行位置

关键事实：**非 `async` 的 `#[tauri::command]` 在主线程（事件循环线程）执行**。一段 10ms 的 SQL 就会掉 1 帧；一次迁移或全量聚合可能几百毫秒，直接界面卡死。

现状里 `commands/*.rs` 全是同步 fn（`learning.rs:21-24`、`knowledge.rs:5-8`），因为它们只返回错误。接入 SQLite 后必须改为 async。

### 2.2 统一模式：async 命令 + `spawn_blocking`

```rust
#[tauri::command]
pub async fn training_submit(
    app: tauri::AppHandle,
    payload: TrainingSubmitPayload,
) -> AppResult<TrainingSubmitResult> {
    let db = db_handle(&app);                       // Arc<Database>，可安全移入闭包
    tauri::async_runtime::spawn_blocking(move || {
        db.with_tx(|tx| training_service::submit(tx, payload))
    })
    .await
    .map_err(|e| AppError::internal(format!("数据库任务失败：{e}")))?
}
```

三条硬约束：

1. **不允许在 `async fn` 里直接调用阻塞 IO**（`Connection::execute` 在 async 上下文会阻塞 tokio worker）。
2. **不允许跨 `await` 持有 `MutexGuard`**。所有数据库操作放在同一个 `spawn_blocking` 闭包里完成；闭包外只传 `Arc`。
3. **`State<'_, _>` 不能移入 `'static` 闭包**，所以要么取 `AppHandle` 再 `app.state::<AppState>()`，要么让 `AppState` 持 `Arc<Database>` 并在命令入口 clone。

### 2.3 逻辑线程图

```
UI 线程 (WebView)
   │ invoke("training_submit", {…})
   ▼
Tauri IPC 派发（async runtime 线程）
   │ spawn_blocking
   ▼
阻塞线程池（tokio blocking pool，默认上限 512）
   │  Mutex<Connection> 加锁 → BEGIN IMMEDIATE → 业务 → COMMIT
   ▼
SQLite（WAL：1 writer + N readers）
```

一次写入的实测预期：36 个实体、单事务 5 条 INSERT + 2 条 UPSERT，**< 5ms**。这个数字必须在阶段 B 用基准测试记录下来（§8），作为"要不要连接池"的依据。

---

## 3. 连接管理

### 3.1 结论

**一个写连接（`Mutex<Connection>`），不引入连接池。**

依据：

| 事实 | 来源 |
|---|---|
| SQLite WAL 允许 1 写 + N 读并发，写操作串行是引擎本身的限制，不是池能绕过的 | SQLite 文档 |
| 本项目写入是低频、短事务：一次训练提交约 5ms | 业务负载（每天几十次） |
| 数据量 ≤ 万级行，查询 < 1ms | `DATA_MODEL_REVIEW.md` §0 |
| 进程内所有访问已由 `Mutex` 串行化，`busy_timeout` 只需处理"多进程/多实例"场景 | §3.3 |

引入 `r2d2` + `r2d2_sqlite` 的收益是"读请求不排队等写锁"，在 5ms 写事务 + 单用户点击频率下，排队概率可以忽略。**没有测量数据就不加池**。

### 3.2 `Database` 结构（与池兼容的接口）

```rust
pub struct Database {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Database {
    /// 只读查询：加锁执行，不开启事务
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T>;

    /// 写操作：BEGIN IMMEDIATE，闭包返回 Err 则整体回滚
    pub fn with_tx<T>(&self, f: impl FnOnce(&Transaction<'_>) -> AppResult<T>) -> AppResult<T>;
}
```

将来若要加池，只改 `Database` 内部（`with_conn` 从池取连接），`repositories/*` 与 `commands/*` 一行不动。这就是"先不做池、但留好接缝"。

### 3.3 PRAGMA 配置

```rust
// 打开连接后立即执行（foreign_keys 是**每连接**设置，不是持久化的）
conn.pragma_update(None, "journal_mode", "WAL")?;      // 持久化到文件，只需成功一次
conn.pragma_update(None, "synchronous", "NORMAL")?;    // WAL 下 NORMAL 已能保证一致性
conn.pragma_update(None, "foreign_keys", "ON")?;       // 每个连接都必须设
conn.busy_timeout(Duration::from_secs(5))?;            // 多实例/后台备份时等待而非立刻失败
conn.pragma_update(None, "temp_store", "MEMORY")?;
conn.pragma_update(None, "cache_size", -16_000)?;      // 16MB
conn.pragma_update(None, "wal_autocheckpoint", 400)?;  // 约 1.6MB 页数触发检查点
```

关于持久性取舍：`synchronous=NORMAL` + WAL 的语义是"崩溃可能丢失最后一两个事务，但不会损坏数据库"。对学习记录来说这是可接受的（丢一个复习动作 vs 卡顿每次 fsync），且**关键写入完成后会触发检查点 + 定期备份**。如果你要求"任何一次提交都不许丢"，改 `synchronous=FULL`，代价是每次提交多一次 fsync（Windows 上约 1–5ms）。**这是一个需要你确认的取舍点**。

### 3.4 数据库文件位置

```rust
let dir = app.path().app_data_dir()?;   // Windows: %APPDATA%\com.sage.learning
std::fs::create_dir_all(&dir)?;
let path = dir.join("sage.db");
```

- 开发期可用 `SAGE_DB_PATH` 环境变量覆盖（便于测试与多套数据并存）。
- **不要在 OneDrive/网络盘存库**：WAL 依赖共享内存映射（`-shm` 文件），网络文件系统上不可靠。启动时若检测到路径包含 `OneDrive`/`Dropbox` 或为 UNC 路径，记一条警告日志（不阻止启动，但在设置页提示）。
- 同目录下还会有 `sage.db-wal`、`sage.db-shm`，备份/清理时要一起处理。

---

## 4. 模块结构

按你建议的 `database/ migrations/ repositories/ models/ commands/ errors/` 组织，另外新增 `services/` 与 `algorithms/`，原因是**业务逻辑不能堆进命令**：

```
src-tauri/src/
├─ main.rs                    保持现状
├─ lib.rs                     Builder：注册状态、命令、启动时初始化数据库
├─ error.rs                   AppError（已有）+ From<rusqlite::Error>
├─ state.rs                   AppState { db: Arc<Database> }（替换内存 KV）
├─ database/
│  ├─ mod.rs                  Database、open、with_conn、with_tx
│  ├─ pragmas.rs              §3.3 的配置
│  ├─ migrate.rs              迁移 runner（§5）
│  ├─ backup.rs               备份 / 恢复 / integrity_check / vacuum
│  ├─ content.rs              内容包同步（§4.4）
│  └─ migrations/
│     ├─ 0001_init.sql        全部 DDL（DATABASE_DESIGN.md §3）
│     └─ 0002_....sql         后续演进
├─ models/                    与 TS 类型一一对应的 serde 结构（camelCase 输出）
│  ├─ mod.rs
│  ├─ entity.rs vocabulary.rs grammar.rs sentence.rs
│  ├─ exercise.rs assessment.rs review.rs plan.rs
│  ├─ user.rs dashboard.rs analytics.rs
├─ repositories/              纯 SQL，无业务规则，无事务管理
│  ├─ mod.rs
│  ├─ entity_repository.rs    内容读取、知识图谱、邻居
│  ├─ state_repository.rs     entity_states 读写、到期查询
│  ├─ training_repository.rs  sessions/answers/kind_scores
│  ├─ assessment_repository.rs
│  ├─ review_repository.rs    review_logs
│  ├─ mastery_repository.rs   mastery_events
│  ├─ plan_repository.rs      study_plans/days/items/goals/advice
│  ├─ user_repository.rs      users/profiles/settings
│  └─ analytics_repository.rs 区间聚合所需的原始行
├─ services/                  事务边界 + 业务编排（**命令只做参数解析与调用**）
│  ├─ training_service.rs
│  ├─ assessment_service.rs
│  ├─ review_service.rs
│  ├─ plan_service.rs
│  ├─ knowledge_service.rs
│  └─ dashboard_service.rs
├─ algorithms/                TS 算法的 Rust 对照实现（纯函数，零 IO）
│  ├─ srs.rs mastery.rs xp.rs grading.rs
│  ├─ ability.rs planner.rs stats.rs selection.rs
├─ content/                   内容包（见 §4.4）
│  └─ content_pack.json       ← 由前端 TS 内容导出生成
└─ commands/
   ├─ app.rs                  已有
   ├─ user.rs                 替换全部 unavailable
   ├─ learning.rs             dashboard/analytics/plan/assessment/training
   ├─ knowledge.rs            graph/list/detail/neighbors
   └─ review.rs               **新增**：review_queue / review_submit
```

### 4.1 分层依赖方向

```
commands → services → repositories → database
                   ↘ algorithms（纯函数）
models ← 所有层
```

禁止：`repositories` 互相调用（必须由 service 编排）、`commands` 里写 SQL、`algorithms` 里访问数据库。

### 4.2 命令层保持"薄"

命令只做三件事：解析参数（反序列化到 `models`）、取 `Arc<Database>`、`spawn_blocking` 调 service。这样 `commands` 文件不会随业务增长而膨胀 —— 现在的 `learning.rs` 已经塞了 dashboard/analytics/plan/assessment/training 五块，实施时建议按域拆到 `review.rs` 之外再拆文件。

### 4.3 仓储层用 trait 还是具体结构

建议**具体结构 + 关联函数**，不上 trait：

```rust
pub struct EntityRepository<'a>(pub &'a Connection);
impl EntityRepository<'_> {
    pub fn find_index(&self, filter: &KnowledgeFilter) -> AppResult<Vec<KnowledgeIndexRow>> { … }
}
```

理由：trait 只在"需要替换实现"时有价值；这里只有一种实现（SQLite）。测试用 `Connection::open_in_memory()` 依然是同一实现，不需要 mock。等真的出现第二种存储（比如同步到云）再抽象。

### 4.4 内容包（关键决策）

**问题**：知识库/题库内容现在只存在于前端 TS（`services/mock/*`）。Rust 要实现 `knowledge_*` / `training_start` / `assessment_modules`，必须能读到这些内容。

**方案（推荐）**：把 TS 内容导出为 JSON，作为构建产物被 Rust 内嵌。

```
src/services/mock/*.ts  ──(npm run export:content)──▶  src-tauri/src/content/content_pack.json
                                                        │ include_str! 编译进二进制
                                                        ▼
                                             database/content.rs：启动时按 id UPSERT 进内容表
```

- `content_pack.json` 含 `version`（内容包版本）+ `entities / vocabulary / grammar / sentences / relations / exercises / training_options / assessment_*`。
- 单一事实源仍是 TS 内容模块（mock 模式继续用它们），JSON 是**生成物**，不允许手改。
- 导出脚本加 `--check` 模式：JSON 与 TS 内容不一致时 CI 失败（避免"改了词库忘了导出"）。
- 内容同步是幂等的：按 id `INSERT ... ON CONFLICT DO UPDATE`，并把 `content_pack_version` 写入 `app_metadata`；版本未变则跳过。
- 词库只增不删：新版本里消失的实体置 `deprecated_at`，不物理删除（历史事件仍引用它）。

**备选方案（不推荐）**：内容继续留在前端，Rust 只存学习态。这样 `knowledge_*` 在 backend 模式下永远 unavailable，等于第二阶段只完成一半。

### 4.4.1 算法也要跟着走

`knowledge_*` 只是读，但训练提交要写掌握度，就必须在 Rust 里有 `mastery/srs/xp/grading`。更进一步，`dashboard_snapshot` 与 `analytics_query` 依赖 `stats/ability`，`plan_*` 依赖 `planner`。

所以 Rust 需要移植 8 个纯函数模块（TS 侧约 1100 行）。这是本阶段**最大的一块工作量**，也是"哪个仓库是权威"的分水岭：

> **Rust 是权威**：写入路径的掌握度、调度、判分、XP 只在 Rust 计算。TS 版本降级为 **mock 适配器 + 对照基准**（golden test 双端跑同一组用例，见 §8）。

如果不移植（而是前端算完再让 Rust 存），会得到：事务失去意义（多步更新被拆成多次 IPC）、backend 模式需要在每次写之前拉全量状态、判分可被前端篡改。**这三条都违背"关键业务操作必须使用事务"的要求。**

---

## 5. 迁移

### 5.1 机制

- 迁移文件：`database/migrations/0001_init.sql`、`0002_xxx.sql`……编号连续，**只增不改**。
- 记录表：`schema_migrations(version, name, checksum, applied_at)`。
- 快速检查：`PRAGMA user_version` 与 `max(version)` 保持一致。
- 编译期内嵌：`include_str!("migrations/0001_init.sql")` 写进 `const MIGRATIONS: &[Migration]`。

### 5.2 启动流程

```
open(path)
 ├─ 若文件不存在 → 新建，直接执行全部迁移
 ├─ 若存在：
 │   ├─ PRAGMA quick_check（怀疑损坏时才做完整 integrity_check）
 │   ├─ 读 max(applied version)
 │   ├─ db_version > 代码版本 → 直接返回 database_too_new 错误，拒绝运行
 │   ├─ db_version < 代码版本 → **先备份**，再逐条执行待应用迁移
 │   └─ 逐条校验 checksum，已应用的脚本被改动 → 报错并提示（不自动修复）
 └─ 设置 PRAGMA，进入服务
```

### 5.3 单条迁移的执行边界

```rust
let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
tx.execute_batch(&migration.sql)?;
tx.execute(
    "INSERT INTO schema_migrations(version, name, checksum, applied_at) VALUES (?1,?2,?3,?4)",
    params![migration.version, migration.name, &checksum, now_iso()],
)?;
tx.pragma_update(None, "user_version", migration.version)?;   // 事务内设置
tx.commit()?;
```

失败处理：

1. 该条迁移回滚（DDL 在 SQLite 里是事务性的，`execute_batch` 失败不会留下半套表）。
2. 关闭连接，**用迁移前的备份文件覆盖回滚**（因为已应用的更早迁移可能已改动数据）。
3. 返回 `AppError::internal`，`detail` 带迁移版本与 SQL 错误；`hint` 指向备份文件路径。

**绝不允许**像现在的前端仓储那样"版本不符就丢弃数据重新种子化"（`repository.ts:38`、`learning-store.ts:52-55`）。

### 5.4 迁移测试

| 用例 | 断言 |
|---|---|
| 空库跑完 `0001` | `user_version = 1`；`sqlite_master` 里 35 表 + 1 视图齐全 |
| 内容包同步两次 | 行数不变（幂等），`content_pack_version` 更新 |
| 带数据升级（v1 → v2 模拟） | 旧行保留；新列有默认值；`schema_migrations` 多一行 |
| 迁移脚本中途失败 | 事务回滚；`user_version` 未变；备份文件存在 |
| 数据库版本高于代码 | 返回明确错误，不修改文件 |

---

## 6. 备份与恢复

### 6.1 备份

```rust
pub fn backup(db: &Connection, dest: &Path) -> AppResult<()> {
    let mut target = Connection::open(dest)?;
    let backup = rusqlite::backup::Backup::new(db, &mut target)?;
    backup.run_to_completion(64, Duration::from_millis(20), None)?;  // 分步复制，不阻塞太久
    Ok(())
}
```

触发时机：

| 时机 | 是否强制 |
|---|---|
| 每次迁移前 | 强制 |
| 应用启动时（≤1 次/天） | 自动，失败不阻止启动但记录 |
| 设置页"立即备份" | 用户触发 |
| `local_data_clear` 前 | 强制（清空不可逆） |

命名：`sage-YYYYMMDD-HHmmss.db`，与主库同目录；保留最近 10 份（同一天最多 3 份），超出按时间删除。

### 6.2 恢复

```
1. 校验目标文件：open → PRAGMA integrity_check → 读 user_version
2. user_version 必须 ≤ 代码版本（更高则拒绝，提示升级应用）
3. 备份当前库（防止用户选错文件）
4. 关闭主连接（Mutex 内 swap）
5. 复制文件 + 删除旧 -wal/-shm
6. 重新 open + 跑迁移 + 重算 app_metadata.db_revision
```

恢复过程中任何一步失败，用第 3 步的备份回滚。UI 只需 toast 提示成功/失败，不需要展示细节。

### 6.3 一致性体检

暴露 `db_maintenance` 命令（见 `DATABASE_DESIGN.md` §8）：

| action | 行为 |
|---|---|
| `integrity_check` | `PRAGMA integrity_check` + 外键 `PRAGMA foreign_key_check` |
| `vacuum` | `VACUUM`（回收空间，会短暂锁库） |
| `backup` | 手动备份 |
| `restore` | 指定路径恢复 |

---

## 7. 错误映射

```rust
impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        use rusqlite::Error::*;
        match err {
            SqliteFailure(e, msg) => match e.code {
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                    => AppError::new("unavailable", "本地数据库正忙，请稍后重试"),
                ErrorCode::ConstraintViolation
                    => AppError::invalid("数据校验未通过（可能重复提交）"),
                ErrorCode::ReadOnly
                    => AppError::new("unavailable", "数据库处于只读状态"),
                ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase
                    => AppError::internal("本地数据库损坏，请从备份恢复"),
                _ => AppError::internal(format!("数据库错误：{}", msg.unwrap_or_default())),
            },
            QueryReturnedNoRows => AppError::not_found("记录"),
            _ => AppError::internal(format!("数据库错误：{err}")),
        }
    }
}
```

约束：

1. `detail` 里可以带 SQL 与参数名，**不能带参数值**（可能有用户输入）。
2. 幂等命中（`UNIQUE(submission_id)` 冲突）**不算错误**：service 捕获 `ConstraintViolation` → 查回既有记录 → 返回 `ok`。前端表现为"重复提交无副作用"（与现在 `duplicate: true` 的语义一致，`learning-db.ts:150-152`）。
3. 所有错误最终序列化为 `"code:message"`（`error.rs:42-57`），前端 `normalizeError` 已经能解析。

---

## 8. 测试策略

### 8.1 Rust 单元测试

- `Connection::open_in_memory()` + 跑全部迁移，每测一个仓储方法。
- `with_tx` 的回滚：闭包返回 `Err` 后断言表未变。
- 幂等：同一 `submission_id` 提交两次，断言行数不变、返回值一致。
- 到期队列：构造 `due_at` 在过去/未来/`NULL` 三种状态，断言只返回到期项且按 `due_at` 升序。
- `local_data_clear`：断言事件表清空、`entity_states` 全部重置、`entities` 与内容表行数不变。

### 8.2 迁移测试

见 §5.4。

### 8.3 双端一致性（golden test）—— 本阶段最关键的一环

Rust 与 TS 各有一套 `srs/mastery/grading/ability/planner` 实现，必须防止漂移：

```
tests/fixtures/parity/
├─ srs_cases.json      输入(state, rating, now) + 期望(ease, intervalDays, dueAt, repetition, lapseCount)
├─ mastery_cases.json  输入(state, partial, difficulty, now) + 期望(mastery, strength, state, factors)
├─ grading_cases.json  输入(exercise kind, value) + 期望(correct, partial)
├─ ability_cases.json  输入(attempts, states) + 期望(axes 分数)
└─ planner_cases.json  输入(profile, ability, states, …) + 期望(items 摘要)
```

同一份 JSON 被：

- `tests/parity/parity.test.ts`（vitest）读取，断言 TS 实现；
- `src-tauri/src/algorithms/parity_tests.rs`（cargo test）读取，断言 Rust 实现。

用例由"现有 TS 实现跑出来的结果"生成（快照即基准），之后任何一侧行为变化都会让另一侧失败。**这是"双实现"唯一可接受的做法。**

### 8.4 基准测试（用于决定是否加连接池）

```rust
#[test]
#[ignore]
fn bench_write_latency() { /* 10k 行数据下，1000 次 review_submit 事务 */ }
```

记录：p50 / p95 / p99 单事务耗时。判定规则：

- p95 < 16ms → **不加池**（当前设计）。
- p95 ≥ 16ms 且热点在读 → 加只读连接池（`r2d2_sqlite`，池大小 4）。
- 只在报告中出现真实数字后才讨论池。

---

## 9. 安全与约束

| 约束 | 做法 |
|---|---|
| SQL 注入 | 全部走 `params![]` / `named_params![]`；禁止 `format!` 拼 SQL。代码评审基线 |
| 前端不接触 SQL | 前端只有命令名（`contracts.ts`），没有 SQL 字符串；这也排除了 `tauri-plugin-sql`（它把 SQL 暴露到 JS） |
| 文件权限 | 数据库在 `app_data_dir`（用户私有目录）。Linux 下 `chmod 600`，Windows 依赖用户 ACL |
| 敏感数据 | 本阶段无账号密码、无网络，`users` 表不含凭证；未来加账号时再引入密钥链（不在本阶段范围） |
| 审计 | 事件表本身可追溯；`app_metadata.db_revision` 便于定位"哪次写入后出的问题" |

---

## 10. 待你确认的技术取舍

| # | 取舍 | 我的建议 | 影响 |
|---|---|---|---|
| R1 | `synchronous=NORMAL` vs `FULL` | NORMAL（崩溃最多丢最后一两个事务，不损坏库） | 每次提交多 1–5ms fsync |
| R2 | 算法移植到 Rust（Rust 权威）vs 前端算完只让 Rust 存 | 移植（否则事务无意义） | 工作量最大，约 1100 行 TS → Rust |
| R3 | 单连接 `Mutex` vs 一开始就上连接池 | 单连接，实测后再定 | 若后续证明不够，只改 `Database` 内部 |
| R4 | 内容包由 TS 导出（生成物入库）vs 内容维护在 Rust | TS 导出 | 需要 `export:content` 脚本 + CI 校验 |
| R5 | 加入 `chrono` + `sha2` 两个新依赖 | 加 | 依赖体积小，收益明确 |
| R6 | 是否引入 `tauri-plugin-single-instance` 防止双开写库 | 建议加（可选） | 避免两个实例各生成一份 active 计划；不加也能靠 WAL + `busy_timeout` 工作 |

---

## 11. 落地核对（2026-11-14）

### 11.1 架构承诺 vs 实际代码

| 承诺 | 实际实现 | 状态 |
|---|---|---|
| 单连接 + `Mutex<Connection>`，不上池 | `Database { conn: Mutex<Connection>, path }`（`database/mod.rs:34-37`） | ✔ 一致 |
| 命令层 `async` + `spawn_blocking` | `commands/mod.rs:22-31` 的 `blocking()` 统一封装；除 `app_*` 外全部 async | ✔ 一致 |
| 仓储只接收 `&Connection`，不感知 `Database` | `repositories/*` 全部签名为 `(&Connection, ..)` | ✔ 一致 |
| 事务统一 `TransactionBehavior::Immediate` | `database/mod.rs:98-114` | ✔ 一致 |
| 迁移"先备份、再执行、失败回滚" | `migrate.rs:90-117` | ✔ 一致 |
| 错误映射 `code:message` 字符串 | `error.rs:60-105` + 前端 `transport.ts:29-63` | ✔ 一致 |
| 内容包由 TS 导出 | `scripts/export-content.mjs` + `content_pack.json` + `npm run check:content` | ✔ 一致 |

### 11.2 仍需补的架构细节

| # | 缺口 | 影响 | 建议 |
|---|---|---|---|
| A1 | `repositories` 无批量查询接口，导致三层 N+1（module→section→exercise、pool→逐题关联、detail→逐 id 内容） | 数据量小尚可，题目/词库增长后明显 | 增加 `sections_of_many(&[ids])`、`find_many(&[ids])` 形态，SQL 用 `IN (...)` 或一次性 JOIN |
| A2 | 内容仓储在模型里硬编码假学习态（`mastery: 0.0, state: "new"`，`entity_repository.rs:240-246` 等 3 处） | 绕过 service 时输出错误掌握度 | 拆 `ContentRow` 与 `ContentRowWithState` 两个类型，编译期强制合成 |
| A3 | `state_repository::upsert` 无条件覆盖 11 个字段并 `row_version+1`，但模型无该字段 | 写放大 + 并发控制失效 | 要么实现 CAS（`WHERE row_version = ?`），要么删列 |
| A4 | 一次训练里同一实体被多次 `upsert`（`training_service.rs:254-291`） | 掌握度被过度加权、事件表膨胀 | 同一会话内先按实体聚合 `partial`，再单次写入 |
| A5 | `with_tx` 对只读补种也 `bump_revision` | 启动即 +1，前端缓存无谓失效 | 补种单独路径，或区分 `maintenance_revision` |
| A6 | `commands/*` 里 6 条命令直接返回 `Err(unavailable)`，无 TODO 标记与跟踪 | 前端以为"功能存在" | 统一在 `API_CONTRACTS.md` 标注，并在 `transport` 按命令声明能力 |
| A7 | 无 Repository 层 trait 抽象（全部具体函数） | 单测依赖真实 SQL，替换实现困难 | **保持现状**：SQLite 内存库测试已经足够快（80 项 < 1s），引入 trait 只会增加噪音 |
| A8 | 备份/恢复/健康检查/prune 未接线 | "看起来有、实际不生效" | 2.4 接线到设置页 + 启动后静默备份 |

### 11.3 线程模型复核要点

1. `spawn_blocking` 的闭包内**必须**重新 `Arc::clone` 拿到 `Database`，不能在主线程持有锁跨越 `await`（现有实现正确，`commands/mod.rs:22-31`）。
2. 单连接 `Mutex` 下，并发的 8 个读命令实际串行。对当前数据量（数百实体）影响可忽略；**一旦 `knowledge_list` 全表载入超过 30ms，就应优先做 A1 的批量查询，而不是上连接池**。
3. 事务内不 `await`、不跨线程；`with_tx` 的回调是同步闭包，编译期即可保证。
4. 崩溃安全：`synchronous=NORMAL` + WAL，最坏丢最后一两个事务，不损坏数据库文件；迁移前快照兜底。

### 11.4 结论

**驱动选型（rusqlite + bundled + backup）与分层（commands / services / repositories / models / database）继续有效，不需要调整。** 下一阶段的工作量在"补业务服务"与"批量查询优化"，不在重构数据层。
