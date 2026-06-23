## ADDED Requirements

### Requirement: Unified player aggregate

系统 MUST 提供统一 `PlayerAggregate` 作为玩家运行态数据的业务聚合，覆盖账号基础信息、对局属性、资产、任务进度、成就进度和统计计数器。业务逻辑 MUST 通过该聚合读取和修改玩家数据，不得直接拼装多个底层 collection 结果作为玩家状态。

#### Scenario: Load complete player aggregate

- **GIVEN** 玩家已存在，并拥有 `users`、`user_inventory`、`user_quest_progress`、`user_achievement_progress` 中的相关数据
- **WHEN** 业务层请求加载该玩家的 `PlayerAggregate`
- **THEN** 系统返回同一玩家 id 下的基础信息、对局属性、资产、任务、成就和统计快照
- **AND** 返回结果中的各子域 MUST 来自同一加载流程，避免调用方自行跨 collection 拼接

#### Scenario: Missing optional domains use defaults

- **GIVEN** 新玩家存在于 `users` 表，但尚无背包、任务或成就进度记录
- **WHEN** 业务层加载该玩家的 `PlayerAggregate`
- **THEN** 系统返回默认空背包、默认任务进度视图、默认成就进度视图和初始统计计数器
- **AND** 加载流程 MUST NOT 因可选子域缺失而失败

### Requirement: Player repository as business entry point

系统 MUST 提供 `PlayerRepository` 作为玩家聚合的唯一业务读写入口。任务、奖励、背包、邀请、商城购买和对局结算等玩家域逻辑 MUST 通过 `PlayerRepository` 修改玩家聚合，避免直接调用 `UserCollection`、`ItemCollection`、`QuestCollection` 等底层 collection 修改同一玩家状态。

#### Scenario: Read-only access does not schedule persistence

- **GIVEN** 调用方只需要展示玩家资料、背包或任务进度
- **WHEN** 调用方通过 `PlayerRepository` 执行只读访问
- **THEN** 系统返回玩家聚合快照
- **AND** 系统 MUST NOT 标记 dirty 分桶
- **AND** 系统 MUST NOT 安排数据库写入

#### Scenario: Mutable access marks affected domains

- **GIVEN** 调用方需要修改玩家 SR、背包数量或任务进度
- **WHEN** 调用方通过 `PlayerRepository` 执行可变访问并提交修改
- **THEN** 系统更新内存中的 `PlayerAggregate`
- **AND** 系统 MUST 标记被修改的 dirty 分桶
- **AND** 调用方 MUST NOT 额外调用底层 collection 的手动持久化方法

### Requirement: Compatibility with existing REST and WebSocket contracts

引入 `PlayerAggregate` 与 `PlayerRepository` MUST 默认保持现有 REST 路由、响应语义和 WebSocket 消息协议兼容。除非本 change 后续任务显式列出协议变更，否则客户端不应感知内部玩家数据存取路径变化。

#### Scenario: Existing REST route reads through repository

- **GIVEN** 客户端调用现有背包、任务、排行榜或用户资料 REST 路由
- **WHEN** 服务端改为通过 `PlayerRepository` 获取玩家数据
- **THEN** HTTP 状态码和 JSON 响应字段 MUST 与变更前兼容
- **AND** 兼容性差异 MUST 在任务验收中列出

#### Scenario: Existing game settlement remains compatible

- **GIVEN** 两名玩家完成一局 WebSocket 对战
- **WHEN** 服务端通过 `PlayerRepository` 更新胜负、SR、任务进度和奖励
- **THEN** 现有可靠传输 ACK、重传、重排序和 `GameOver` 消息语义 MUST 保持兼容
- **AND** 内部聚合更新失败 MUST NOT 破坏 WebSocket 连接管理状态
