## ADDED Requirements

### Requirement: Unified player activity events

系统 MUST 提供统一的玩家行动事件抽象，覆盖至少以下行为类型：注册、登录、登出、离线、购买、使用物品、完成任务、领取任务奖励、添加好友、完成关卡。每条事件 MUST 至少包含行为类型与发生时间，并 SHOULD 关联 `user_id`（注册前/匿名事件允许 `user_id` 为空）。

#### Scenario: Record a player action as an event

- **GIVEN** 玩家发生了一项受支持的行为（例如购买、完成关卡）
- **WHEN** 业务逻辑提交该行为
- **THEN** 系统 MUST 生成一条包含行为类型与发生时间的行动事件
- **AND** 该事件 SHOULD 关联触发该行为的 `user_id`

### Requirement: Non-blocking recording

行动日志记录 MUST 以非阻塞方式提交，MUST NOT 阻塞或拖慢触发该行为的主业务流程（如登录、购买、对局结算）。

#### Scenario: Logging does not block business flow

- **GIVEN** 某业务流程在执行中需要记录行动日志
- **WHEN** 业务提交行动事件
- **THEN** 提交 MUST 立即返回而不等待数据库写入
- **AND** 主业务流程的结果 MUST NOT 因日志写入而改变

### Requirement: Batched persistence on size or interval

系统 MUST 将行动事件缓冲后批量写入数据库：当缓冲累计达到 100 条时 MUST 立即落库；否则 MUST 至少每 5 秒落库一次（两者先到者触发）。

#### Scenario: Flush when buffer reaches threshold

- **GIVEN** 行动事件缓冲尚未到达定时落库时刻
- **WHEN** 缓冲累计达到 100 条
- **THEN** 系统 MUST 立即将这批事件批量写入数据库

#### Scenario: Flush on interval when below threshold

- **GIVEN** 缓冲中存在未落库事件但不足 100 条
- **WHEN** 距上次落库已达 5 秒
- **THEN** 系统 MUST 将缓冲中的事件批量写入数据库

### Requirement: Weak consistency on persistence failure

行动日志为非资产数据。落库失败时系统 MUST NOT 影响主业务，SHOULD 有限次重试，超过上限后 MAY 丢弃并记录告警。

#### Scenario: Persistence failure does not affect business

- **GIVEN** 行动日志批量落库失败
- **WHEN** 系统处理该失败
- **THEN** 系统 MUST NOT 回滚或阻塞任何主业务操作
- **AND** 系统 SHOULD 重试有限次后丢弃并记录告警日志
