# mini-game-server

通用小游戏后端服务，使用 **Rust + Actix-web + sqlx + PostgreSQL 16** 构建。

提供账号/平台登录、好友、道具背包与商城、任务与成就、排行榜、SR 天梯、通知收件箱、玩家聚合缓存，以及一套**可靠 WebSocket 实时链路**。当前承载的具体小游戏是**英歌破鬼阵（`game_key = yingge`）**——一款关卡/分数/星级制的关卡游戏，客户端为 darkHero（Cocos Creator 3.8.8）。项目同时保留了来自 `fourinarow-server` 的实时对战（lobby / room）链路作为通用 room 能力的演进基底。

> 代码 crate 名仍为 `fourinarow-server`（v1.4.0，历史原因保留），但项目定位是**通用小游戏后端**，不是“四子棋对战服务”。

---

## 技术栈

| 维度 | 选型 |
|------|------|
| 语言 | Rust（edition **2024**，需 Rust ≥ 1.85；CI 镜像锁定 `rust:1.96-bookworm`） |
| Web / Actor | Actix-web 4 · actix 0.13 · actix-web-actors 4 · tokio 1 |
| 数据库 | PostgreSQL 16 · sqlx 0.7（`query!`/`query_as!` 编译期校验） |
| 实时通信 | WebSocket + 自研可靠传输层（ACK / 重传 / 重排序 / 重连） |
| 静态资源 | `actix-files` 暴露 `static/` |
| 其他 | reqwest（平台登录回调）· lettre（Gmail 反馈邮件）· dashmap（玩家缓存）· sha2/sha3（配置 checksum / 口令哈希） |

---

## 目录结构

```
src/
  main.rs              服务启动：加载 GameConfig、DB、各 actor、路由、静态资源
  config/              GameConfig（启动时 fail-fast 加载 config/*.json）
  api/                 REST 层（协议解析 / 鉴权 / 参数校验 / 调用下层）
    auth/              匿名登录（device_id 免密建号）
    platform/          微信 / 抖音小游戏 OAuth 登录
    users/             账号、session、好友
    inventory.rs       背包 / 商城
    quests.rs          主线 / 每日 / 成就任务
    leaderboard.rs     RPG 排行榜（SR / 胜场）
    invites.rs         邀请码
    chat/ inbox.rs notifications.rs   社交 / 收件箱 / 红点
    gameplay.rs        complete_level（关卡结算）
    minigame_config.rs       小游戏配置版本化下发
    minigame_leaderboard.rs  小游戏关卡分数榜
    minigame_tasks.rs        小游戏任务（签到 / 每日 / 成就）
  game/                WebSocket 实时链路
    msg.rs             消息协议、序列化、解析
    client_adapter.rs  可靠层：ACK / 重传 / 重排序
    client_connection.rs  WS session actor
    connection_mgr.rs  连接态与重连
    lobby.rs lobby_mgr.rs  房间 / 匹配 / 对局 / 结算状态机
  database/            sqlx repository（每张业务表一个文件）
  player/              玩家聚合缓存：aggregate / repository（dirty bucket + write-behind + 幂等）
  items/ quests/       静态物品 / 任务定义
  logging/             活动日志、对局日志
migrations/            schema 唯一真相（001..013，启动时自动迁移）
config/                小游戏配置 JSON（关卡 / 场景 / 模式）
static/                静态页面（index / admin / privacy / terms）
deploy/                生产 DB 初始化与导入导出脚本
docs/                  项目文档（prod / implement_sum 等）
agents/                服务端开发 agent 规范
openspec/              范围治理（changes / specs / archive）
```

---

## 快速开始

### 1. 准备 PostgreSQL 16

本地开发使用 Docker（容器名 / 库名以你的环境为准）：

```bash
# 若尚无容器（按需调整名称/密码）
docker run -d --name postgres -p 5432:5432 \
  -e POSTGRES_PASSWORD=postgres postgres:16-alpine

# 建库
docker exec -it postgres psql -U postgres -c "CREATE DATABASE fourinarow;"
```

> ⚠️ 启动 / 停止 Docker 容器属高风险操作，按项目规范需先与维护者确认。

### 2. 配置环境变量

```bash
cp .env_template .env
# 编辑 .env，至少填好 DATABASE_URL
```

### 3. 运行

```bash
cargo run                 # 默认监听 127.0.0.1:7060
# 或指定地址
BIND=127.0.0.1:7060 RUST_LOG=info cargo run
```

启动时会：加载 `config/*.json`（失败即退出）→ 连接 DB → 执行未应用的 migration（`sqlx::migrate!`）→ 拉起各 actor → 监听 HTTP/WS。

访问 `http://127.0.0.1:7060/` 会重定向到 `static/index.html`。

---

## 配置（环境变量）

复制 `.env_template` 为 `.env` 后填写。`.env` 已被 `.gitignore` 忽略，不入库。

| 变量 | 说明 | 默认 / 示例 |
|------|------|------|
| `DATABASE_URL` | PostgreSQL 连接串 | `postgres://postgres:postgres@localhost:5432/fourinarow` |
| `BIND` | 监听地址 | 代码默认 `127.0.0.1:7060`；Docker 镜像默认 `0.0.0.0:7060`；compose 用 `0.0.0.0:8080` |
| `RUST_LOG` | 日志级别 | `actix_web=info` |
| `MINIGAME_CONFIG_ADMIN_TOKEN` | 小游戏配置**写接口**鉴权令牌；**为空则写接口一律拒绝（fail-closed）**，生产填强随机值 | `change_me` |
| `WECHAT_APPID` / `WECHAT_SECRET` | 微信小游戏凭据 | — |
| `DOUYIN_APPID` / `DOUYIN_SECRET` | 抖音小游戏凭据 | — |
| `GMAIL_MAIL_FROM` / `GMAIL_MAIL_TO` / `GMAIL_PW` | 反馈邮件（Gmail SMTP） | — |
| `PLAYER_CACHE_FLUSH_IMMEDIATELY` | 玩家缓存同步刷盘开关，设 `0`/`false` 启用延迟 write-behind | 默认开启（同步） |

平台密钥、Gmail 凭据、admin token 一律来自环境变量，**不得硬编码**。

---

## 数据库与迁移

- **schema 唯一真相是 `migrations/`**；服务启动时 `sqlx::migrate!("./migrations")` 自动执行未应用的迁移。
- **改表结构必须新增 migration 文件**（如 `014_*.sql`），**绝不修改已存在的旧 migration**。
- `query!` / `query_as!` 为编译期校验：
  - 在线：`DATABASE_URL` 指向可连接的 PostgreSQL，`cargo check` 时连库校验。
  - 离线：先 `cargo sqlx prepare` 生成 `.sqlx` 缓存即可脱库编译（CI 镜像构建用）。

| 迁移 | 内容 |
|------|------|
| `001_initial` | `users` / `auth_identities` 等核心表（账号、平台身份、SR） |
| `002_items_inventory_shop` | `items` / `user_inventory` / 商店 + 种子（coin/gem/ticket） |
| `003_quests` | `quests` / `user_quest_progress` / 成就阶梯 |
| `004_invites` | `invite_codes` / `invite_code_uses` |
| `005_player_cache_management` | `player_stats` / `player_operation_idempotency` / `asset_ledger`（幂等 + 资产流水） |
| `006_realtime_and_progression` | `activity_log` / `player_mode_progress` |
| `007_quests_and_progressive_achievements` | 任务与渐进成就扩展 |
| `008_notification_inbox_badge` | 通知收件箱 / 红点 |
| `009_minigame_config` | 小游戏配置版本表 `minigame_config_version` / `_active` |
| `010_seed_minigame_yingge_config_v1` | 英歌小游戏 v1 配置种子（关卡/场景/模式整包） |
| `011_minigame_leaderboard` | `minigame_level_score`（每关最佳分/星） |
| `012_minigame_tasks` | 小游戏每日活跃 / 签到 / 领取记账 / 成就进度 |
| `013_minigame_signin_streak` | 签到连续数 `streak` |

---

## REST API 一览

所有接口挂在 `/api` scope 下。登录态接口通过 **`SessionToken` 请求头**鉴权。

**账号 / 登录**
- `POST /api/auth/anonymous` — 匿名登录（`device_id` 免密建号，幂等返回同账号）
- `POST /api/platform/wechat/login` · `POST /api/platform/douyin/login` — 小游戏平台登录
- `POST /api/users/register` · `POST /api/users/login` · `POST /api/users/logout`
- `GET /api/users/me` · `GET /api/users/{user_id}` · `GET /api/users?...`（搜索）

**好友**
- `POST /api/users/me/friends` · `DELETE /api/users/me/friends/{id}`

**资产 / 任务 / 排行榜（RPG 域）**
- `GET /api/inventory` · `GET /api/inventory/shop/{shop_id}` · `POST /api/inventory/shop/{shop_id}/buy/{item_id}`（支持 `Idempotency-Key`）
- `GET /api/quests/story` · `/daily` · `/achievements`（+ 领奖）
- `GET /api/leaderboard?type=skill_rating|wins&page=N` · `GET /api/leaderboard/me`
- `GET /api/invites` · `POST /api/invites` · `POST /api/invites/redeem`

**社交 / 通知**
- `GET|POST /api/chat/{thread_id}`
- `GET /api/inbox` · `POST /api/inbox/{id}/claim` · `/read` · `DELETE /api/inbox/{id}`
- `GET /api/notifications/badges` · `POST /api/notifications/badges/{module}/clear`
- `POST /api/feedback`（触发反馈邮件）

**关卡结算**
- `POST /api/game/complete_level` — `{ mode, level_id, stars }` → 结算并发奖

**小游戏配置版本化下发（`/api/minigame-config`）**
- `GET /{game_key}/manifest` — 当前现役版本清单
- `GET /{game_key}/active` — 现役整包（支持 `ETag` → `304`）
- `GET /{game_key}/versions` · `GET /{game_key}/versions/{version}`
- `POST /{game_key}/versions` · `POST /{game_key}/versions/{version}/publish` · `POST /{game_key}/rollback` — **写接口需 admin token（fail-closed）**

**小游戏排行榜与任务（`/api/minigame`）**
- `POST /{game_key}/score` — 提交关卡分数/星级
- `GET /{game_key}/leaderboard` · `GET /{game_key}/leaderboard/me`
- `GET /{game_key}/tasks` — 三类任务 + 进度
- `POST /{game_key}/tasks/{task_id}/claim` — 领取（一次性 / 轮次推进）
- `POST /{game_key}/tasks/event/signin` — 签到上报

---

## WebSocket 实时链路

- 路径：`GET /game/`（升级为 WebSocket）。
- **可靠传输层**（`client_adapter.rs`）在普通 WS 之上保证送达与顺序：
  - `ACK::<id>` — 确认对端消息
  - `MSG::<id>::<payload>` — 带序号的业务消息
  - `ERR::<reason>` — 错误（如 `INVALID_FORMAT` / `UNKNOWN_MESSAGE` / `KILL_CLIENT`）
  - `HELLO::...` — 握手（下发 session token、是否新连接）
- 业务消息见 `src/game/msg.rs`：上行 `PlayerMessage`、下行 `ServerMessage` / `GameMsgOut`，覆盖关卡列表分页、`CompleteLevel`、好友上线、成就解锁、排行榜更新、收件箱新消息等。
- 房间 / 对局状态机由 `lobby_mgr` 调度，断线重连由 `connection_mgr` + 可靠层处理。

> 改动 `msg.rs` 协议时，必须同步检查 `client_adapter.rs`（可靠层）与客户端契约，避免协议不兼容。

---

## 玩家聚合缓存

- 玩家资产 / 任务 / 结算等读写优先通过 `src/player/repository.rs` 的 `PlayerRepository`。
- 内存态按字段分桶标脏（dirty bucket），后台 flush worker 周期写回 PostgreSQL（默认 500ms tick / 5s cooldown / 10min 强制刷盘）；下线、正常退出时强制 flush。
- 写路径保证**幂等**：商城购买、邀请兑换、对局结算在单事务内完成，并用 `player_operation_idempotency` + `asset_ledger` 防重复发奖 / 重复流水。
- 详见 [docs/implement_sum.md](docs/implement_sum.md)。

---

## 构建与验证

```bash
cargo check                 # 最低交付门槛（连库或已 sqlx prepare）
cargo build
cargo build --release       # 发布前
cargo build --release --target x86_64-unknown-linux-musl   # 交叉编译，见 cross_compile.md
```

无单元测试框架，行为改动需给手动验证步骤：启动服务后用 `load_test.html` 或 WS 客户端走登录 / 连接 / 关卡结算 / 断线重连流程；REST 用 curl 验证。

---

## 部署

- **Docker / compose**：`docker-compose.yml` 拉起 `server`（多阶段 Dockerfile）+ `postgres:16-alpine`，通过 Traefik 标签反向代理。

  ```bash
  docker compose up -d --build
  docker compose logs -f server
  ```

- **CI**：`.github/workflows/docker-publish.yml` 构建镜像并推送 GHCR。
- **systemd**：`fourinarow-server.service`（`systemctl enable/start`）。
- **生产 DB**：`deploy/01_prod_db_setup.sql` 初始化，`deploy/02_export.sh` / `03_restore.sh` 导入导出。

详见 [infrastructure.md](infrastructure.md) 与 [docs/prod.md](docs/prod.md)。

> ⚠️ 生产部署、交叉编译产物发布、Docker 启停、改库属高风险操作，执行前需先确认。

---

## 开发规范

本项目同时启用 **OpenSpec（范围治理）** 与 **Superpowers（执行纪律）**。规范优先级：`AGENTS.md` > `CLAUDE.md` > 环境变量 > 对话明确指示。

- **必须先走 OpenSpec change** 的场景：新功能 / 功能域扩展、REST 合同变化、WS 协议变化、数据库 schema/migration 变化、权限/部署/环境变量变化、SR 与结算等核心业务规则变化。
- **可直接改**：文档 / 注释 / 日志文案、不改外部行为的小 bugfix 与内部重构。
- 遵循 `api` / `game` / `database` / `player` 分层，不跨层耦合。
- 文档统一放 `docs/`（注意 `.gitignore` 当前忽略 `docs/`，入库需先确认）。

关键文档：
- [AGENTS.md](AGENTS.md) — 项目最高优先级规范
- [CLAUDE.md](CLAUDE.md) — OpenSpec + Superpowers 工作流
- [agents/game-server-developer.md](agents/game-server-developer.md) — 服务端开发 agent
- [infrastructure.md](infrastructure.md) — 基础设施 / 端口 / 部署
- [docs/implement_sum.md](docs/implement_sum.md) — 玩家缓存与业务实现摘要
- [cross_compile.md](cross_compile.md) — musl 交叉编译
