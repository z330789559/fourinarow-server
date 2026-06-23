## ADDED Requirements

### Requirement: Player cache with read and write intent

系统 MUST 为 `PlayerAggregate` 提供缓存访问语义，区分只读访问和可变访问。只读访问 MUST 无副作用；可变访问或显式提交修改 MUST 标记对应 dirty 分桶，并安排延迟持久化。

#### Scenario: Read-only aggregate access

- **GIVEN** 玩家聚合已存在于缓存中
- **WHEN** 调用方以只读模式获取该玩家聚合
- **THEN** 系统返回缓存中的玩家快照
- **AND** 系统 MUST NOT 更新 dirty 分桶
- **AND** 系统 MUST NOT 改变 flush 时间戳

#### Scenario: Mutable aggregate access

- **GIVEN** 玩家聚合已存在于缓存中
- **WHEN** 调用方以可变模式修改玩家资产或任务进度
- **THEN** 系统 MUST 更新缓存中的玩家聚合
- **AND** 系统 MUST 标记受影响的 dirty 分桶
- **AND** 系统 MUST 记录本次修改时间

#### Scenario: Cache miss loads from database

- **GIVEN** 玩家聚合不在缓存中
- **WHEN** 调用方请求该玩家聚合
- **THEN** 系统 MUST 从 PostgreSQL 加载玩家数据
- **AND** 系统 MUST 将加载结果放入缓存
- **AND** 如果是只读访问，系统 MUST NOT 标记 dirty

### Requirement: Dirty buckets for player domains

系统 MUST 按玩家数据域维护 dirty 分桶，至少覆盖 `profile`、`game_info`、`inventory`、`quests`、`achievements`、`stats`。flush MUST 只写回被标记的分桶，避免未变化数据被重复写入。

#### Scenario: Inventory change marks only inventory bucket

- **GIVEN** 玩家通过任务奖励获得道具
- **WHEN** 系统更新玩家背包数量
- **THEN** 系统 MUST 标记该玩家的 `inventory` dirty 分桶
- **AND** 系统 MUST NOT 因本次修改标记 `profile` 或 `game_info` 分桶

#### Scenario: Settlement marks multiple buckets

- **GIVEN** 玩家完成一局对战并触发 SR、统计、任务和奖励变化
- **WHEN** 系统提交对局结算修改
- **THEN** 系统 MUST 标记 `game_info`、`stats`、`quests` 和必要的 `inventory` 分桶
- **AND** 所有分桶 MUST 归属于同一玩家聚合版本

### Requirement: Deferred flush tick

系统 MUST 提供后台 flush tick，将 dirty 分桶在冷却时间后批量写回数据库。默认冷却时间 SHOULD 为 5 秒，tick 间隔 SHOULD 为 500 毫秒；具体值可以通过常量或配置调整，但 MUST 在设计与测试中明确。

#### Scenario: Flush after cooldown

- **GIVEN** 玩家 `inventory` 分桶在 6 秒前被标记 dirty
- **WHEN** flush tick 运行且冷却时间为 5 秒
- **THEN** 系统 MUST 将该玩家的 `inventory` 分桶写回 PostgreSQL
- **AND** 写入成功后 MUST 清除该分桶 dirty 状态

#### Scenario: No flush before cooldown

- **GIVEN** 玩家 `quests` 分桶在 3 秒前被标记 dirty
- **WHEN** flush tick 运行且冷却时间为 5 秒
- **THEN** 系统 MUST 保留该分桶 dirty 状态
- **AND** 系统 MUST NOT 提前写入数据库

### Requirement: Force flush and lifecycle flush

系统 MUST 提供强制刷盘机制，确保持续被修改的玩家数据不会无限延迟写入。系统 MUST 在玩家下线、连接结束、服务关闭或显式维护操作时支持 `flush_player` 或 `flush_all`。

#### Scenario: Force flush hot player

- **GIVEN** 玩家聚合持续被修改，导致普通冷却窗口不断刷新
- **WHEN** 距离该分桶上次成功落库超过 10 分钟
- **THEN** 系统 MUST 强制写回该 dirty 分桶
- **AND** 成功后 MUST 更新最后落库时间

#### Scenario: Flush player on logout

- **GIVEN** 玩家存在 dirty 分桶
- **WHEN** 玩家登出或 WebSocket 连接结束并释放玩家运行态
- **THEN** 系统 MUST 尝试 flush 该玩家所有 dirty 分桶
- **AND** flush 失败时 MUST 记录错误并保留可恢复状态

#### Scenario: Flush all on shutdown

- **GIVEN** 服务收到正常关闭信号
- **WHEN** HTTP server 和 actor 系统停止前进入清理阶段
- **THEN** 系统 MUST 调用 `flush_all`
- **AND** 系统 MUST 等待 dirty 玩家写回完成或达到明确的超时策略

### Requirement: Failed flush retains dirty state

系统 MUST 在数据库写入失败时保留 dirty 状态，并允许后续 tick 或手动恢复再次尝试。系统 MUST 记录失败原因，避免静默丢失玩家变更。

#### Scenario: Database error during flush

- **GIVEN** 玩家 `inventory` 分桶处于 dirty 状态
- **WHEN** flush tick 写入 PostgreSQL 失败
- **THEN** 系统 MUST 保留 `inventory` dirty 分桶
- **AND** 系统 MUST 记录包含玩家 id、分桶名和错误原因的日志
- **AND** 后续 tick MUST 再次尝试写回
