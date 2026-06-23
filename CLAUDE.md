本项目同时使用 OpenSpec 和 Superpowers。

> ⚠️ 本文件与 `AGENTS.md` 内容重复时，**以 `AGENTS.md` 为准**（AGENTS.md 是项目最高优先级规范）。

规则：
1. 所有新功能、架构变化、数据库变化（migration）、API 合同变化、WebSocket 消息协议变化、权限变化，必须先走 OpenSpec change。
2. OpenSpec 负责定义范围、requirements、scenarios、design 和 tasks。
3. Superpowers 负责 brainstorming、TDD、systematic debugging、code review。
4. 实现时必须严格对照当前 active OpenSpec change，不得新增未定义功能。
5. 如果发现需求缺失或 spec 与代码冲突，必须先暂停实现并报告。
6. 修复普通 bug 可以直接使用 Superpowers systematic debugging，但不得修改 OpenSpec 范围。
7. change 完成后必须先验证和验收，再 archive。
8. 所有 shell 和 md 文档都必须放在 `docs/` 文件夹下面，除非特别说明。

铁律：每次运行必须先记住并遵守以下项目结构，不得混淆路径。
- 当前 Rust 工程：`/Users/libaozhong/game/mini-game-server/`
- 最高优先级规范：`AGENTS.md`；本文件 `CLAUDE.md`（OpenSpec + Superpowers）
- 基础设施说明：`infrastructure.md`
- 文档：`docs/`（`prod.md` 部署、`implement_sum.md` 实现总结等）
- 数据库迁移：`migrations/`（`001..004_*.sql`，schema 唯一真相，启动自动 `sqlx::migrate!`）
- OpenSpec：`openspec/`（范围治理）
- 静态资源：`static/`
- 源码模块：`src/api`（REST）`src/game`（WS actor 对战）`src/database`（sqlx）`src/items` `src/quests` `src/logging`
- 数据库：PostgreSQL 16（Docker），sqlx 编译期校验

---

## 1. 工作语言

只用中文交流解释、计划、总结；代码标识符与技术术语保留英文。

## 2. 项目概况

Rust 2021 / Actix-web 4（actor 模型）/ sqlx 0.7 + PostgreSQL 16 / WebSocket 实时四子棋对战 + 自研可靠传输层。
功能涵盖账号、好友、邀请码、道具/背包/商店、任务、SR 天梯，平台接入微信 / 抖音小游戏。
详见 `AGENTS.md` 第 2 节项目画像。

## 3. Claude 工作流

- 复杂任务先 Sequential Thinking 列计划；用 `rg` / Serena 定位代码。
- 涉及 `AGENTS.md` 第 4 节场景，先用 OpenSpec 定义 change：
  - CLI：`openspec`（`openspec validate --all` / `openspec status` / 新建 change）
  - 入口：`/opsx` 系列命令
- 用 OpenSpec 定义范围后，再用 Superpowers（TDD、systematic debugging、code review）实现与验证。
- 查 Actix / sqlx / tokio 等库用法时优先 Context7 MCP，不凭记忆。

## 4. 代码与生成链路

- 检索优先 `rg`，符号理解用 Serena。
- sqlx 链路：`query!` / `query_as!` 编译期校验，字段须与 `migrations/` 当前 schema 一致；脱库编译需 `cargo sqlx prepare`（`.sqlx` 缓存）。
- **不盲改 migration**：表结构变更一律新增 migration 文件，绝不修改已存在的旧 migration。

## 5. 项目特定规则

- 遵循 api / game / database 分层，不跨层耦合。
- 改 `src/game/msg.rs`（WS 协议）须同步检查 `src/game/client_adapter.rs`（可靠层 ACK/重传/重排序）与客户端契约。
- 改 lobby 状态机（`lobby.rs` / `lobby_mgr.rs` / `connection_mgr.rs`）须考虑并发与断线重连。
- 平台密钥、Gmail 凭据等来自环境变量，不得硬编码。

## 6. 验证

- `cargo check`（快）→ `cargo build` → `cargo build --release`（发布前）。
- 无测试框架，行为改动须给手动验证步骤（启动服务 + `load_test.html` 或 WS 客户端走流程）。

## 7. 禁止事项

- ❌ 修改已存在的旧 migration 文件
- ❌ 硬编码密码 / 平台密钥（用 `.env`）
- ❌ 未确认即执行高风险操作（删库/改表/生产接口/真实邮件/git commit·push/Docker 启停/部署）—— 详见 `AGENTS.md` 第 10 节
- ❌ 破坏 Obsidian 符号链接同步
