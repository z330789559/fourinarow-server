# Design — minigame-config-version-service

## 分层与放置

沿用现有 `api / database` 分层，不引入新抽象：

- `src/database/minigame_config.rs`：sqlx 查询（编译期校验），所有 SQL 收口于此。
- `src/api/minigame_config.rs`：actix handler + `pub fn config(cfg: &mut web::ServiceConfig)`，在 `src/api/mod.rs` 以
  `.service(web::scope("/minigame-config").configure(minigame_config::config))` 挂载，最终前缀 `/api/minigame-config`。
- handler 签名沿用现有风格：`async fn xxx(req: HttpRequest, db: web::Data<Arc<DatabaseManager>>) -> HttpResponse`，DB 走 `db.pool`。

## 数据库（migration `009_minigame_config.sql`）

```sql
-- 版本化配置包：一版一行，三份配置整包存 JSONB
CREATE TABLE minigame_config_version (
    id            BIGSERIAL   PRIMARY KEY,
    game_key      TEXT        NOT NULL,                     -- 'yingge'，预留多小游戏
    channel       TEXT        NOT NULL DEFAULT 'production', -- 'production' | 'staging'
    version       INTEGER     NOT NULL,                     -- (game_key, channel) 内单调递增
    status        TEXT        NOT NULL DEFAULT 'draft',     -- 'draft' | 'published' | 'archived'
    level_config  JSONB       NOT NULL,                     -- = MinigameYinggeLevelConfigData
    scene_config  JSONB       NOT NULL,                     -- = MinigameYinggeSceneConfigData
    mode_config   JSONB       NOT NULL,                     -- = MinigameYinggeModeConfigData
    checksum      TEXT        NOT NULL,                     -- sha256(规范化三份拼接)，ETag + 完整性
    note          TEXT,
    created_by    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at  TIMESTAMPTZ,
    CONSTRAINT uq_mcv UNIQUE (game_key, channel, version)
);
CREATE INDEX idx_mcv_lookup ON minigame_config_version (game_key, channel, status);

-- 现役版本指针：每个 (game_key, channel) 一条
CREATE TABLE minigame_config_active (
    game_key   TEXT        NOT NULL,
    channel    TEXT        NOT NULL,
    version_id BIGINT      NOT NULL REFERENCES minigame_config_version(id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (game_key, channel)
);
```

- JSONB 列在 Rust 侧映射为 `serde_json::Value`；`query!`/`query_as!` 编译期校验需可连库或 `cargo sqlx prepare` 生成 `.sqlx`。
- 不做按关卡/障碍规范化建表：数据 ~20KB 且客户端整包消费，规范化只增发版与拼包复杂度，无收益；JSONB 仍可按需 SQL 内省。

## REST 合同

基址 `/api/minigame-config`。`channel` 默认 `production`。

### 只读（公开）

| 方法 / 路径 | 说明 |
|---|---|
| `GET /{game_key}/manifest?channel=` | 返回现役版本号 + checksum + published_at（几十字节），供客户端廉价判断是否需要下整包 |
| `GET /{game_key}/active?channel=` | 返回现役整包；响应头带 `ETag: "<checksum>"` 与 `Cache-Control`；请求带 `If-None-Match` 命中则回 `304 Not Modified` |
| `GET /{game_key}/versions/{version}?channel=` | 返回指定版本整包（客户端 pin 版本 / 回滚验证用） |

manifest 响应：
```jsonc
{ "gameKey": "yingge", "channel": "production", "version": 7,
  "checksum": "sha256:ab12…", "publishedAt": "2026-06-27T10:00:00Z" }
```
active / by-version 响应：
```jsonc
{ "gameKey": "yingge", "channel": "production", "version": 7, "checksum": "sha256:ab12…",
  "level": [ /* …LevelConfigData 原样 */ ],
  "scene": [ /* …SceneConfigData 原样 */ ],
  "mode":  [ /* …ModeConfigData 原样  */ ] }
```
错误：无 active 版本 → `404`；非法 channel / version → `400`。

### 管理（需 admin token）

| 方法 / 路径 | 说明 |
|---|---|
| `POST /{game_key}/versions` | body=`{channel, level, scene, mode, note}` → 新建 **draft** 版本：服务端校验三份均为 JSON 数组、算 checksum、`version = max(version)+1`，返回 `{version, checksum}` |
| `POST /{game_key}/versions/{version}/publish?channel=` | 事务内：置该版本 `status=published`+`published_at`，并 upsert `minigame_config_active` 指向它，返回新 active |
| `POST /{game_key}/rollback?channel=&to=` | 事务内：校验目标版本存在且 `status=published`，把 active 指向它 |
| `GET /{game_key}/versions?channel=` | 版本历史列表（不含整包，含 version/status/checksum/note/created_at/published_at）；管理用，需 token |

## 鉴权

- admin token：读 `std::env::var("MINIGAME_CONFIG_ADMIN_TOKEN")`。请求头 `x-admin-token: <token>` 与之**常量时间**比较。
- **fail closed**：env 未设置或为空 → 所有写接口一律 `503`/`401`（拒绝），绝不放行。
- 读接口公开，不校验。沿用现有 CORS 配置；如客户端域名不同需在 `/api` CORS 增加来源（实现时确认，不在本 change 扩 CORS 语义）。
- 与现有 `session_token`（玩家会话）相互独立：配置发布是运营动作，不复用玩家会话。

## 配置（环境变量）

- `.env_template` 增加：`MINIGAME_CONFIG_ADMIN_TOKEN=change_me`（开发默认占位；生产填强随机值）。
- 不硬编码任何 token。

## 种子（version 1 与现状一致）

- 把 darkHero 现役三份 JSON 作为 fixture 纳入后端仓库：`docs/seed/minigame/yingge/{level,scene,mode}.json`。
- 提供种子方式（实现时二选一并在 tasks 标注，需用户确认后执行写库）：
  - A) 部署后用管理接口 `POST /yingge/versions`（channel=production）+ `publish` 写入 v1；或
  - B) 一段一次性 seed SQL（从 fixture 读入），仅在 `minigame_config_version` 为空时插入，避免重复。
- 任一方式都必须保证：上线后 `GET /api/minigame-config/yingge/active` 返回的整包与 darkHero 内置 JSON 等价。

## checksum 规范

- `checksum = "sha256:" + hex(sha256( canonical(level) ++ "\n" ++ canonical(scene) ++ "\n" ++ canonical(mode) ))`。
- `canonical` = 稳定序列化（键有序、无多余空白），保证同内容跨语言/跨次一致，可作 ETag 与客户端完整性校验。
- 依赖：crate 已有 `sha3`；sha256 用 `sha2`（若未引入则在 tasks 说明新增依赖理由，遵守"不无故加依赖")。

## 安全 / 性能 / 可靠性

- 安全：写接口 token + fail closed；handler 校验 game_key/channel 取值白名单，避免任意写入；JSONB 入库前校验为数组。
- 性能：读多写极少；manifest 极小；active 用 ETag/304 省带宽。Phase 1 直查库，必要时后续加 active 内存缓存（带 checksum 失效）。
- 可靠性：publish/rollback 单事务完成"选版本 + 翻指针"，杜绝 active 指向不存在或未发布版本；版本只增不删（archived 状态软处理）。

## 回滚

- 配置层：`rollback` 接口把 active 指回上一个 published 版本即可，秒级、无需改代码。
- 代码层：功能为纯新增；如需整体停用，仅需在 `src/api/mod.rs` 不挂载该 scope。
- 数据层：migration 仅新增两表，不影响既有 schema。

## 跨工程衔接（非本 change 范围，仅记录）

- darkHero 客户端的"在线优先 + localStorage 缓存 + 内置 JSON 兜底"读取改造，另立 darkHero 侧 OpenSpec change，引用本 change 的 manifest/active 合同。
- 编辑器"应用正式配置"改为调本服务管理接口（Phase 2）。
