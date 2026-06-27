# minigame-config-admin-api Specification

## Purpose
TBD - created by archiving change minigame-config-version-service. Update Purpose after archive.
## Requirements
### Requirement: Admin token authentication with fail-closed

写接口（建版本、发布、回滚、历史列表）MUST 校验来自环境变量 `MINIGAME_CONFIG_ADMIN_TOKEN` 的管理令牌，通过请求头 `x-admin-token` 提交并做常量时间比较。当该环境变量未设置或为空时，系统 MUST 拒绝所有写接口（fail closed），绝不放行。

#### Scenario: Reject when token missing or wrong

- **GIVEN** 服务已配置 `MINIGAME_CONFIG_ADMIN_TOKEN`
- **WHEN** 调用任一写接口时未带 `x-admin-token` 或值不匹配
- **THEN** 系统 MUST 返回 401 且不产生任何写入

#### Scenario: Fail closed when token unconfigured

- **GIVEN** 服务未设置 `MINIGAME_CONFIG_ADMIN_TOKEN`
- **WHEN** 调用任一写接口（即使带了某个 token）
- **THEN** 系统 MUST 拒绝该请求，不执行任何写入

### Requirement: Create draft version

系统 MUST 提供 `POST /api/minigame-config/{game_key}/versions`（需 admin token）接收 `{channel, level, scene, mode, note}` 新建 `draft` 版本：校验 level/scene/mode 均为 JSON 数组、`channel ∈ {production, staging}`、计算 checksum、分配递增版本号，返回新版本号与 checksum。新建 draft MUST NOT 改变现役指针。

#### Scenario: Create draft does not affect active

- **GIVEN** `(yingge, production)` 现役为版本 7
- **WHEN** 通过该接口新建一个 draft 版本 8
- **THEN** 系统返回 `version=8` 与其 checksum
- **AND** 现役指针 MUST 仍指向版本 7

#### Scenario: Reject invalid payload

- **GIVEN** 请求体中 `level` 不是 JSON 数组
- **WHEN** 调用建版本接口
- **THEN** 系统 MUST 返回 400 且不创建版本

### Requirement: Publish version atomically

系统 MUST 提供 `POST /api/minigame-config/{game_key}/versions/{version}/publish?channel=`（需 admin token），在单个数据库事务内将目标版本置为 `published` 并把现役指针指向它。事务失败时 MUST 不留下"已置 published 但指针未更新"或反之的中间态。

#### Scenario: Publish flips active pointer

- **GIVEN** `(yingge, production)` 存在 draft 版本 8，现役为版本 7
- **WHEN** 发布版本 8
- **THEN** 版本 8 状态 MUST 为 published
- **AND** 现役指针 MUST 指向版本 8

### Requirement: Rollback to a published version

系统 MUST 提供 `POST /api/minigame-config/{game_key}/rollback?channel=&to=`（需 admin token），在事务内把现役指针指向一个已 `published` 的历史版本。目标版本不存在或未发布时 MUST 拒绝且不改变现役指针。

#### Scenario: Rollback restores previous version

- **GIVEN** `(yingge, production)` 现役为版本 8，版本 7 为 published
- **WHEN** 回滚到 `to=7`
- **THEN** 现役指针 MUST 指向版本 7

#### Scenario: Reject rollback to unpublished version

- **GIVEN** 版本 9 状态为 draft
- **WHEN** 回滚到 `to=9`
- **THEN** 系统 MUST 拒绝（4xx）且现役指针保持不变

### Requirement: List version history

系统 MUST 提供 `GET /api/minigame-config/{game_key}/versions?channel=`（需 admin token）返回该 `(game_key, channel)` 的版本历史元信息（version、status、checksum、note、created_at、published_at），MUST NOT 在列表中返回整包内容。

#### Scenario: History excludes bundle payload

- **GIVEN** `(yingge, production)` 有版本 1..8
- **WHEN** 管理端请求版本历史
- **THEN** 系统返回各版本的元信息列表
- **AND** 响应 MUST NOT 包含 level/scene/mode 整包数据

