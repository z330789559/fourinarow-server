# AGENTS.md — 项目最高优先级规范（mini-game-server）

> 本文件是本工程对所有 AI 编码代理（Claude Code、Codex 等）的**最高优先级规范**。
> 与其他文档（含 CLAUDE.md）冲突时，**以本文件为准**。
> 全局基础设施标准见 `~/.codex/INFRASTRUCTURE.md` 与 vault `00-Inbox/ai/infrastructure-standards.md`，本项目数据库以**项目实际（PostgreSQL/sqlx）**为准。

---

## 1. 核心原则

1. **只用中文交流**：所有解释、计划、总结使用中文；代码标识符、技术术语保留英文。
2. **质量第一**：正确性 > 速度。宁可慢，不要留下隐患。
3. **先理解再修改**：动手前先读懂相关模块（api / game / database）和数据流，不臆测。
4. **小步可追踪**：每次改动聚焦一个目标，便于 review 和回滚。
5. **完整交付**：改完要能 `cargo check` 通过，并给出验证步骤；不交付半成品。
6. **保护用户改动**：不覆盖、不删除用户未要求变更的代码与配置。
7. **不留无用兼容分支**：重构时清理被替代的死代码，不堆叠“以防万一”的旧逻辑。

---

## 1.1 专用 Agent 配置

- 服务端开发专用 agent 文档：`agents/game-server-developer.md`。
- 当任务涉及服务端功能设计、实现、排查、重构、OpenSpec change 执行或通用小游戏能力演进时，必须先读取该文档。
- `agents/game-server-developer.md` 的定位是：本项目是 Rust 版**通用小游戏后端服务**，当前代码基底来自 `fourinarow-server`，不得把项目目标描述成“四子棋实时对战服务”。
- 若 `agents/game-server-developer.md` 与本文件冲突，仍以本文件为准；若本文件未覆盖的服务端执行细节，以该 agent 文档为补充规范。

---

## 2. 项目画像

| 维度 | 实际值 |
|------|--------|
| 定位 | Rust 版通用小游戏后端服务，当前代码基底来自 `fourinarow-server` |
| 语言 | Rust 2021 edition，当前 crate 名称 `fourinarow-server` v1.4.0 |
| Web/Actor | Actix-web 4 + actix-web-actors 4 + actix 0.13 + tokio 1 |
| 数据库 | PostgreSQL 16，sqlx 0.7（`postgres` + `macros` + `migrate` + `chrono`，编译期校验） |
| 实时通信 | WebSocket 实时通信 / 状态同步 + 自研可靠传输层（消息送达保证、重排序、自动重连） |
| 模块 | `src/api`（REST）`src/game`（WS actor / room / 状态同步演进）`src/database`（sqlx）`src/items` `src/quests` `src/logging` `src/player` |
| 迁移 | `migrations/001..004_*.sql`，启动时 `sqlx::migrate!` 自动执行 |
| 测试 | 无单元测试框架；仅 `load_test.html` 手动压测 |
| 现有文档 | `README.md` `docs/prod.md` `docs/implement_sum.md` `cross_compile.md` `agents/game-server-developer.md` |

**业务目标**：沉淀通用小游戏后端能力，当前已包含账号系统、好友、邀请码、道具/背包/商店、任务、排行榜、SR/积分、可靠 WebSocket 通信和玩家聚合缓存；后续会扩展状态同步、AI/AIO、状态机、瓦片地图初始化与管理、技能机制、技能设计、公会等能力，平台接入微信 / 抖音小游戏。

---

## 3. 工具优先级

1. **`rg`（ripgrep）** — 文本检索的默认工具，优先于 `grep`/`find`。
2. **Serena 符号检索** — 跨文件理解符号、调用关系、定位定义。
3. **Sequential Thinking** — 复杂任务的分步推理与计划。
4. **Context7 MCP** — 查 Actix-web / actix / sqlx / tokio 等库的**当前文档**（API、配置、迁移），优先于凭记忆作答。
5. **Web 搜索** — 以上都无法解决时的兜底。

---

## 4. OpenSpec 与 Superpowers 协作

本项目同时启用 **OpenSpec（范围治理）** 与 **Superpowers（执行纪律）**。

**必须先走 OpenSpec change 的场景：**
- 新功能 / 功能域扩展
- REST API 合同变化（`src/api` 路由、请求/响应结构）
- WebSocket 消息协议变化（`src/game/msg.rs` 及客户端可靠层契约）
- 数据库 schema 变化（新增/修改 `migrations/`）
- 权限、部署、配置（环境变量）变化

**可直接改（无需 OpenSpec）：**
- 文档、注释、日志文案
- 不改变对外行为的小 bugfix 与重构
- 不涉及协议/schema 的内部实现微调

**协作分工：**
- OpenSpec 定义范围：proposal / specs / scenarios / design / tasks。
- Superpowers 负责执行：brainstorming、TDD、systematic debugging、code review。
- 实现必须严格对照当前 active change，不新增未定义功能；发现需求缺失或 spec 与代码冲突，先暂停并报告。

---

## 5. 修改原则

- **遵循分层**：`api`（对外接口）/ `game`（对战 actor 与状态）/ `database`（持久化）职责清晰，不跨层耦合。
- **改 WS 协议须检查可靠层**：改动 `src/game/msg.rs` 的消息类型，必须同步检查 `src/game/client_adapter.rs`（可靠传输 ACK / 重传 / 重排序）与客户端契约，避免协议不兼容。
- **改 sqlx 查询须与 schema 对齐**：`query!` / `query_as!` 是编译期校验的，字段名、类型必须与 `migrations/` 当前 schema 完全一致。
- **新依赖须说明**：引入新 crate 前说明理由、体积/维护性影响，优先复用现有依赖。
- **lobby 状态**：改动 `lobby.rs` / `lobby_mgr.rs` / `connection_mgr.rs` 的状态机，须考虑并发与断线重连路径。

---

## 6. 生成链路 / 数据库链路

- **schema 源头是 `migrations/`**：表结构以 migration 文件为唯一真相。
- **改表结构必须新增 migration**，绝不修改已存在的旧 migration 文件（启动 `sqlx::migrate!` 已应用，篡改会破坏迁移一致性）。
- **sqlx 编译期校验**依赖数据库连接或离线缓存：
  - 在线：`DATABASE_URL` 指向可连接的 PostgreSQL，`cargo check` 时连库校验。
  - 离线：执行 `cargo sqlx prepare` 生成 `.sqlx` 缓存后可脱库编译。
- 详细基础设施见 `infrastructure.md`。

---

## 7. 项目业务实践

- **GameId / 用户 id**：用户 id 为 `VARCHAR(12)`；对局/邀请码等有各自生成规则，沿用现有逻辑，勿擅改格式。
- **Lobby 状态机**：匹配 → 房间建立 → 对战 → 结算，由 `lobby_mgr` 调度，单个 `lobby` 持有一局；断线由 `connection_mgr` + 可靠层处理重连。
- **可靠传输**：`client_adapter` 负责 ACK / retry / 重排序，保证消息送达与顺序；新增消息须接入该机制。
- **SR 天梯**：`users.skill_rating`（默认 1000），结算后更新；改动算法须走 OpenSpec。
- **平台 OAuth**：微信 / 抖音小游戏通过 `auth_identities`（provider + openid）与 `users.source_platform` 关联；密钥来自环境变量 `WECHAT_*` / `DOUYIN_*`，不得硬编码。

---

## 8. 文档规则

- 正式文档统一写入 `docs/`，Markdown + 中文。
- 专用 agent 文档统一写入 `agents/`，当前入口为 `agents/game-server-developer.md`。
- 注意：本项目 `.gitignore` 当前忽略了 `docs/`（不入库）；如需文档入库，先与用户确认是否调整忽略规则。
- 临时分析/计划也放 `docs/`，除非用户另有指定。

---

## 9. 验证标准

- 编译校验：`cargo check`（快）→ `cargo build` → `cargo build --release`（发布前）。
- 单条命令尽量短、聚焦；长输出先小范围验证。
- 无测试框架，故对行为改动须给**手动验证步骤**（例：启动服务后用 `load_test.html` 或 WS 客户端走一遍对战流程）。
- sqlx 相关改动，确保编译期校验通过（在线连库或已 `cargo sqlx prepare`）。

---

## 10. 高风险操作（执行前必须确认）

以下操作必须先向用户说明并取得确认：
- 删除数据库 / 表、修改表结构、批量修改数据
- 调用生产接口、发送真实邮件（Gmail SMTP）
- `git commit` / `git push` / `git reset --hard` / 删除分支
- 启动 / 停止 Docker 容器或数据库
- 交叉编译产物的部署（musl / systemd / Traefik）

**确认格式：**
```
⚠️ 高风险操作确认
操作：<要做什么>
影响：<可能后果 / 影响范围>
是否继续？(需要你明确同意)
```

---

## 11. 推荐执行流程

1. **读任务** — 明确目标、范围、验收标准。
2. **定位** — 用 `rg` / Serena 找到相关模块与调用链。
3. **计划** — 复杂任务先列步骤；涉及第 4 节场景先走 OpenSpec。
4. **小步改** — 单一目标的小改动。
5. **验证** — `cargo check` + 手动验证步骤。
6. **总结** — 中文说明改了什么、为什么、如何验证、有何风险。
