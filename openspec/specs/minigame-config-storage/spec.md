# minigame-config-storage Specification

## Purpose
TBD - created by archiving change minigame-config-version-service. Update Purpose after archive.
## Requirements
### Requirement: Versioned config bundle storage

系统 MUST 以 `(game_key, channel, version)` 为唯一单位存储小游戏配置整包，每个版本包含 level、scene、mode 三份 JSON（JSONB）以及内容 checksum。版本号 MUST 在同一 `(game_key, channel)` 内单调递增，且历史版本 MUST 保留不被覆盖删除。

#### Scenario: Create new version increments sequentially

- **GIVEN** `(game_key=yingge, channel=production)` 已存在版本 1 与 2
- **WHEN** 系统为该 `(game_key, channel)` 新建一个配置版本
- **THEN** 新版本号 MUST 为 3
- **AND** 版本 1 与 2 的内容 MUST 保持不变

#### Scenario: Same game across channels keeps independent versions

- **GIVEN** `(yingge, production)` 已有版本 5
- **WHEN** 在 `(yingge, staging)` 新建第一个版本
- **THEN** 该 staging 版本号 MUST 为 1，与 production 的版本序列互不影响

### Requirement: Active version pointer

系统 MUST 为每个 `(game_key, channel)` 维护唯一的现役版本指针，指向某个已存在的版本行。读取"现役配置"MUST 通过该指针解析，指针 MUST NOT 指向不存在或未发布的版本。

#### Scenario: Active pointer resolves current bundle

- **GIVEN** `(yingge, production)` 的现役指针指向版本 7
- **WHEN** 业务层请求该 `(game_key, channel)` 的现役整包
- **THEN** 系统返回版本 7 的 level/scene/mode 整包与其 checksum

#### Scenario: No active pointer yields empty result

- **GIVEN** `(yingge, staging)` 尚无现役指针
- **WHEN** 业务层请求其现役整包
- **THEN** 系统返回"无现役版本"，调用方据此返回 404

### Requirement: Content checksum integrity

系统 MUST 为每个版本计算并存储基于规范化序列化的内容 checksum（`sha256:` 前缀），相同内容 MUST 得到相同 checksum，用作 HTTP ETag 与客户端完整性校验。

#### Scenario: Identical content yields identical checksum

- **GIVEN** 两次提交的 level/scene/mode 内容在语义上完全相同（仅键顺序或空白差异）
- **WHEN** 系统分别计算两者 checksum
- **THEN** 两个 checksum MUST 相等

