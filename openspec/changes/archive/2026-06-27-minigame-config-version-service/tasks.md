## 1. Schema 与种子数据准备

- [x] 1.1 确认当前最新 migration 为 `008_*`，新增 `migrations/009_minigame_config.sql`（`minigame_config_version` + `minigame_config_active`），不修改任何旧 migration
- [x] 1.2 把 darkHero 现役三份 JSON 复制为 fixture：`docs/seed/minigame/yingge/{level,scene,mode}.json`（+ `checksum.txt`）
- [x] 1.3 选定种子方式：**B（seed migration `010_seed_minigame_yingge_config_v1.sql`，dollar-quote 内嵌 + NOT EXISTS 幂等）**
- [x] 1.4 应用 009/010（`cargo sqlx migrate run`），`psql \d` 验证两表结构与索引/约束正确

## 2. 数据库层 `src/database/minigame_config.rs`

- [x] 2.1 读模型 `ConfigBundle / ConfigManifest / ConfigVersionMeta`（JSONB 用 `::text`→`serde_json::Value`，避免开 sqlx json feature）
- [x] 2.2 `get_active(game_key, channel)`（join active 指针）
- [x] 2.3 `get_manifest(game_key, channel)`（仅 version/checksum/published_at）
- [x] 2.4 `get_version(game_key, channel, version)`
- [x] 2.5 `create_draft(...)`（子查询 `MAX(version)+1`，插入 draft，写入用 `$n::jsonb` 绑定 canonical 字符串）
- [x] 2.6 `publish(...)`（事务：置 published + upsert active 指针）
- [x] 2.7 `rollback(...)`（事务：校验目标已 published，翻 active 指针）
- [x] 2.8 `list_versions(...)`
- [x] 2.9 `cargo check` 通过（runtime sqlx，不依赖在线宏校验）

## 3. checksum 与校验工具

- [x] 3.1 `canonical_json`（serde_json 默认键有序紧凑 UTF-8）+ `compute_checksum`（`sha256:` 前缀）与种子 python canonical 对齐
- [x] 3.2 入参校验：level/scene/mode 必须为 JSON 数组；channel ∈ {production, staging}；game_key 字符集白名单
- [x] 3.3 新增依赖 `sha2 = "0.10"`（理由：内容 checksum/ETag，与种子一致），`cargo check` 通过

## 4. 鉴权 admin token

- [x] 4.1 `.env_template` + 本地 `.env` 增加 `MINIGAME_CONFIG_ADMIN_TOKEN`
- [x] 4.2 `require_admin(req)`：读 env token，比对 header `x-admin-token`，常量时间比较
- [x] 4.3 fail closed（env 空 → 503）；未带/错 token → 401（联调已验 401）

## 5. API 层 `src/api/minigame_config.rs`

- [x] 5.1 `pub fn config(cfg)` 注册路由；`src/api/mod.rs` 挂载 `web::scope("/minigame-config")`
- [x] 5.2 `GET /{game_key}/manifest`（无 active → 404）
- [x] 5.3 `GET /{game_key}/active`（`ETag`/`Cache-Control`；`If-None-Match` 命中 → 304）
- [x] 5.4 `GET /{game_key}/versions/{version}`
- [x] 5.5 `POST /{game_key}/versions`（admin，建 draft）
- [x] 5.6 `POST /{game_key}/versions/{version}/publish`（admin，事务）
- [x] 5.7 `POST /{game_key}/rollback`（admin，事务）
- [x] 5.8 `GET /{game_key}/versions`（admin，历史列表）
- [x] 5.9 错误映射：400 / 401 / 404 / 503

## 6. 种子写入 v1

- [x] 6.1 010 写入 `yingge` production version 1（来自 fixture）并置 published + active
- [x] 6.2 验证：`GET /yingge/active` 整包 = darkHero 内置（checksum `63a1ddc0…` 与离线计算一致；level 100 / scene 100 / mode 1）

## 7. 集成验证与文档

- [x] 7.1 `cargo check` 通过；`cargo run` 成功构建并启动（隐含 build 通过）
- [x] 7.2 手动联调（curl）：manifest → active(200/ETag) → 304 → 建 draft(staging) → publish → 回滚 → 渠道隔离 → 全部符合预期
- [x] 7.3 鉴权验证：无/错 token → 401；token 未配置 fail closed（代码路径 503）
- [x] 7.4 `docs/frontend_integration.md` 增加本服务 REST 合同段落
- [x] 7.5 用户已验收（2026-06-27），执行 `openspec archive minigame-config-version-service`（验证已过 `--strict`）

## 回滚 / 恢复

- 配置异常：`POST /{game_key}/rollback?to=<上一个 published 版本>` 秒级回退 active。
- 功能停用：`src/api/mod.rs` 移除该 scope 挂载即可（纯新增）。
- schema：009/010 仅新增表与种子；撤销由新增 migration 处理，绝不改既有 migration。
