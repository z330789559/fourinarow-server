# infrastructure.md — 基础设施说明（mini-game-server）

> 本项目数据库以**项目实际（PostgreSQL 16 + sqlx）**为准，不套用全局 MySQL 标准。
> 全局基础设施标准见末尾「全局参考」。

---

## 快速参考

| 服务 | 镜像 / 容器 | 端口 | 用户 | 密码 | 库名 |
|------|------------|------|------|------|------|
| PostgreSQL（开发） | `postgres`（标准容器） | 5432 | postgres | postgres | fourinarow |
| PostgreSQL（compose） | `postgres:16-alpine` | 5432（expose） | fourinarow | fourinarow | fourinarow |

连接串：
- 本地开发：`postgres://postgres:postgres@localhost:5432/fourinarow`
- docker-compose 内部：`postgres://fourinarow:fourinarow@postgres:5432/fourinarow`

> 实际取值以 `.env` 的 `DATABASE_URL` 为准；`.env_template` 提供模板。

---

## 端口表

| 用途 | 地址 | 说明 |
|------|------|------|
| 本地开发服务 | `127.0.0.1:40146` | 通过 `BIND` 环境变量配置 |
| Docker / 生产服务 | `0.0.0.0:8080` | Dockerfile 默认 `ENV BIND=0.0.0.0:8080` |
| PostgreSQL | `5432` | 数据库端口 |

---

## 环境变量模板

来自 `.env_template`（复制为 `.env` 后填值；`.env` 已被 `.gitignore` 忽略，不入库）：

```bash
# PostgreSQL 连接串
DATABASE_URL=postgres://postgres:postgres@localhost:5432/fourinarow

# 本地绑定地址（开发用，不在模板中但代码读取）
BIND=127.0.0.1:40146
# 日志级别
RUST_LOG=actix_web=info

# 微信小游戏凭据
WECHAT_APPID=your_wechat_appid
WECHAT_SECRET=your_wechat_secret

# 抖音小游戏凭据
DOUYIN_APPID=your_douyin_appid
DOUYIN_SECRET=your_douyin_secret

# 邮件反馈（Gmail SMTP）
GMAIL_MAIL_FROM=your@emailaddress.com
GMAIL_MAIL_TO=your@emailaddress.com
GMAIL_PW=yourVeryGudPassword
```

---

## Docker / docker-compose

`docker-compose.yml` 启动两个服务：

- **server**：`build: .`（多阶段 Dockerfile），`DATABASE_URL` 指向内部 `postgres`，`BIND=0.0.0.0:8080`，通过 Traefik 标签反向代理到 `fourinarow.ffactory.me`。
- **postgres**：`postgres:16-alpine`，账号/密码/库均为 `fourinarow`，数据卷 `./db`，自带 `pg_isready` 健康检查；server `depends_on` 其 healthy。

常用命令：
```bash
# 启动（前台）
docker compose up --build
# 启动（后台）
docker compose up -d --build
# 查看 server 日志
docker compose logs -f server
# 进入数据库
docker compose exec pg psql -U fourinarow -d fourinarow
# 停止
docker compose down
```

> ⚠️ 启动 / 停止容器属高风险操作，执行前需用户确认。

---

## 数据库迁移与 sqlx 编译期校验

- **schema 唯一真相**：`migrations/001..004_*.sql`。
- **自动迁移**：服务启动时 `sqlx::migrate!` 自动执行未应用的迁移。
- **改表结构**：必须**新增** migration 文件（如 `005_*.sql`），**绝不**修改已存在的旧 migration。
- **编译期校验**：`query!` / `query_as!` 在 `cargo check` 时校验 SQL，依赖：
  - 在线：可连接的 `DATABASE_URL`；或
  - 离线：先运行 `cargo sqlx prepare` 生成 `.sqlx` 缓存后可脱库编译。

当前迁移概览：
| 文件 | 内容 |
|------|------|
| `001_initial.sql` | users / auth_identities 等核心表（用户、平台身份、SR） |
| `002_items_inventory_shop.sql` | items / user_inventory / 商店 |
| `003_quests.sql` | quests / user_quest_progress（任务） |
| `004_invites.sql` | invite_codes / invite_code_uses（邀请码） |

---

## 构建与运行

```bash
# 开发构建/检查
cargo check
cargo build

# 运行（开发）
RUST_LOG=actix_web=info cargo run --release

# 发布构建
cargo build --release

# 交叉编译（musl，详见 cross_compile.md）
cargo build --release --target x86_64-unknown-linux-musl
```

---

## 部署

- **Dockerfile**：多阶段（`rust:buster` 构建 → `debian:bullseye-slim` 运行），复制 release 二进制 + `static/` + `.env`。
- **musl 交叉编译**：见 `cross_compile.md`。
- **systemd**：`fourinarow-server.service`（`systemctl enable/start fourinarow-server`）。
- **Traefik**：反向代理，域名 `fourinarow.ffactory.me`，由 compose labels 配置。

> ⚠️ 生产部署、交叉编译产物部署属高风险操作，执行前需用户确认。

---

## 数据库操作规则

- ❌ **禁止**手动建表 / 手动改表结构（一律走新增 migration）。
- ✅ **允许**直接修改数据（测试 / 调试用）。
- ✅ 表结构变更通过新增 `migrations/*.sql`，由启动时 `sqlx::migrate!` 统一执行。
- ⚠️ 删库、改表、批量改数据、连接生产库前，**必须先与用户确认**。

---

## 全局参考

- 全局基础设施标准：`~/.codex/INFRASTRUCTURE.md`
- vault 文档：`00-Inbox/ai/infrastructure-standards.md`
- 注：全局标准中 PostgreSQL 容器为 `postgres` / 5432 / postgres·postgres，与本项目开发环境一致；compose 内使用 `fourinarow` 账号为项目实际。
