## Why

darkHero 小游戏（首个为「英歌破鬼阵」）当前的关卡、场景、模式配置是打包进客户端的静态 JSON
（`assets/resources/configData/MinigameYingge{Level,Scene,Mode}ConfigData.json`）。改一次配置就要重新发版客户端，
运营无法热更新关卡/数值；编辑器产出的 draft 也只能靠人工跑脚本写回源文件，链路脆弱且不可追踪、不可回滚。

需要由 `mini-game-server` 提供**版本化的小游戏配置服务**：客户端启动时从接口拉取现役配置，后端发布新版本后客户端即可获取最新版本，
并保留全部历史版本以支持回滚。配置体量很小（三份 JSON 合计 ~20KB），且客户端按"整包"消费，因此采用**版本化 JSON 包**存储，
而非按关卡/障碍规范化建表。命名空间用 `game_key` 预留多小游戏，符合本项目"通用小游戏后端"的定位。

## What Changes

- 新增 `minigame_config` 能力域：以 `(game_key, channel, version)` 为单位存储整包配置（level/scene/mode 三份 JSONB）+ checksum，
  用一个 `active` 指针表标记每个 `(game_key, channel)` 的现役版本。
- 新增**只读 REST 接口**（公开，供游戏客户端调用）：取现役版本清单（manifest，仅版本号+checksum，便于廉价版本协商）、
  取现役整包（active，支持 `If-None-Match`/`ETag` → 304）、取指定版本整包。
- 新增**管理 REST 接口**（需 admin token）：新建草稿版本、发布版本（置 published 并原子翻转 active 指针）、回滚到指定历史版本、列出版本历史。
- 新增 channel 概念：`staging` 与 `production`，支持"先发 staging 自测、验证后再 publish 到 production"。
- 新增 admin 鉴权：写接口校验 `MINIGAME_CONFIG_ADMIN_TOKEN`（来自 `.env`），未配置则写接口一律拒绝（fail closed）；读接口公开。
- 新增 migration `009_minigame_config.sql`（仅新增表，不改旧 migration）。
- 提供 `yingge` 的 version 1 种子数据（来自 darkHero 现役三份 JSON），保证服务上线即与客户端现状一致。
- 不在本 change 内做：客户端（darkHero）拉取/缓存/兜底改造（另立 darkHero 侧 OpenSpec change）、编辑器发布链路改造（Phase 2）、
  管理后台 UI（Phase 3）、配置 schema 的服务端深度校验/规范化建表、CDN/对象存储分发、灰度按用户分流。

## Capabilities

### New Capabilities

- `minigame-config-storage`：定义版本化配置包的存储模型、`(game_key, channel, version)` 唯一性、active 指针语义与 checksum 完整性。
- `minigame-config-read-api`：定义客户端只读接口（manifest / active / by-version）、ETag/304 缓存协商与公开访问行为。
- `minigame-config-admin-api`：定义管理接口（建版本 / 发布 / 回滚 / 历史）、admin token 鉴权、发布与回滚的原子性与权限边界。

### Modified Capabilities

- 无。当前仓库尚无主规格文件，本 change 以新增能力定义后续实现合同。

## Impact

- 新增代码：`src/api/minigame_config.rs`（REST 路由与 handler）、`src/database/minigame_config.rs`（sqlx 读写）。
- 修改代码：`src/api/mod.rs`（挂载 `/minigame-config` scope）；必要时 `src/database/mod.rs` 暴露新模块。
- 新增 migration：`migrations/009_minigame_config.sql`（`minigame_config_version`、`minigame_config_active` 两张表）；不修改任何旧 migration。
- 新增环境变量：`MINIGAME_CONFIG_ADMIN_TOKEN`（`.env` / `.env_template`），写接口鉴权用，禁止硬编码。
- 对外兼容：纯新增 REST 接口与表，不改动现有 REST 路由、WebSocket 协议或既有表，向后兼容。
- 安全：写接口必须 admin token 校验且 fail closed；读接口公开但只暴露配置数据，不含用户隐私。
- 性能：配置整包 ~20KB；manifest 接口仅回版本号+checksum，客户端靠 ETag/304 避免重复下载；读多写极少，可后续加内存缓存。
- 可靠性：发布与回滚在单事务内"写/选版本 + 翻 active 指针"，避免出现指针指向不存在或未发布版本的中间态。
- 回滚策略：所有版本留库；线上配置异常时用 rollback 接口把 active 指回上一个已发布版本即可，无需改代码或迁移；migration 仅新增表，禁用功能只需不挂载路由。
