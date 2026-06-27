## ADDED Requirements

### Requirement: Public manifest endpoint

系统 MUST 提供公开只读接口 `GET /api/minigame-config/{game_key}/manifest?channel=`，返回现役版本号、checksum 与发布时间，且不返回整包内容，供客户端廉价地判断是否需要下载整包。`channel` 缺省 MUST 为 `production`。

#### Scenario: Manifest returns current version meta

- **GIVEN** `(yingge, production)` 现役为版本 7
- **WHEN** 客户端请求 `GET /api/minigame-config/yingge/manifest`
- **THEN** 响应 MUST 包含 `version=7` 与该版本 checksum
- **AND** 响应体 MUST NOT 包含 level/scene/mode 整包数据

#### Scenario: Manifest without active version returns 404

- **GIVEN** `(yingge, staging)` 无现役版本
- **WHEN** 客户端请求该 channel 的 manifest
- **THEN** 系统 MUST 返回 404

### Requirement: Public active bundle endpoint with ETag negotiation

系统 MUST 提供公开只读接口 `GET /api/minigame-config/{game_key}/active?channel=` 返回现役整包，响应 MUST 带 `ETag` 为该版本 checksum；当请求携带 `If-None-Match` 且与现役 checksum 匹配时，系统 MUST 返回 `304 Not Modified` 且不带整包体。

#### Scenario: Full bundle on first fetch

- **GIVEN** 客户端无缓存（不带 `If-None-Match`）
- **WHEN** 请求 `GET /api/minigame-config/yingge/active`
- **THEN** 系统返回 200 与完整 level/scene/mode 整包
- **AND** 响应头 MUST 含 `ETag` 等于该版本 checksum

#### Scenario: Not modified when checksum matches

- **GIVEN** 客户端持有现役版本的 checksum
- **WHEN** 请求 active 并带 `If-None-Match: <该 checksum>`
- **THEN** 系统 MUST 返回 304 且不返回整包体

### Requirement: Public version pinning endpoint

系统 MUST 提供公开只读接口 `GET /api/minigame-config/{game_key}/versions/{version}?channel=` 返回指定版本整包，供客户端 pin 版本或验证回滚目标。请求不存在的版本 MUST 返回 404。

#### Scenario: Fetch specific historical version

- **GIVEN** `(yingge, production)` 存在版本 5
- **WHEN** 请求 `GET /api/minigame-config/yingge/versions/5`
- **THEN** 系统返回版本 5 的整包，即使现役指针指向其它版本
