## ADDED Requirements

### Requirement: Player mutations are atomic at business operation level

系统 MUST 将一次玩家业务操作中的相关修改作为一个一致性单元处理。商店购买、任务奖励、邀请兑换和对局结算等操作 MUST 避免出现部分成功状态，例如货币已扣但道具未发、任务已标记领奖但奖励未入账。

#### Scenario: Shop purchase succeeds atomically

- **GIVEN** 玩家拥有足够货币并购买商城道具
- **WHEN** 系统执行购买操作
- **THEN** 系统 MUST 同时完成扣除货币、增加商品、更新库存或购买记录
- **AND** 玩家聚合中的资产变化 MUST 与数据库最终状态一致

#### Scenario: Shop purchase failure leaves no partial mutation

- **GIVEN** 玩家发起购买且过程中库存更新或数据库写入失败
- **WHEN** 系统返回购买失败
- **THEN** 系统 MUST NOT 留下货币已扣但商品未发的状态
- **AND** 系统 MUST 保留可重试或可恢复的错误记录

### Requirement: Rewards are idempotent

系统 MUST 为任务奖励、邀请奖励、活动奖励和对局奖励提供幂等标识。重复请求、断线重连、flush 重试或服务重启恢复 MUST NOT 导致同一奖励重复发放。

#### Scenario: Task reward retry does not duplicate items

- **GIVEN** 玩家完成任务并获得奖励事务号
- **WHEN** 奖励发放请求因网络或数据库错误被重试
- **THEN** 系统 MUST 根据幂等标识识别同一奖励
- **AND** 玩家最多获得一次该奖励

#### Scenario: Invite redemption is single-use per redeemer

- **GIVEN** 玩家已成功兑换某邀请码
- **WHEN** 相同玩家重复提交兑换请求或请求被并发处理
- **THEN** 系统 MUST 只记录一次兑换
- **AND** 系统 MUST 只发放一次兑换奖励

### Requirement: Settlement updates are consistent

系统 MUST 在对局结算时一致地更新胜负双方的 SR、游戏记录、统计计数器、任务进度和奖励。若结算无法完整提交，系统 MUST 有明确的回滚、重试或补偿策略。

#### Scenario: Successful game settlement

- **GIVEN** 一局对战产生 winner 和 loser
- **WHEN** 系统处理 `PlayedGame` 结算
- **THEN** winner 的 SR、统计、任务进度和奖励 MUST 更新
- **AND** loser 的 SR、统计和任务进度 MUST 更新
- **AND** 游戏记录 MUST 与双方玩家状态一致

#### Scenario: Settlement failure is recoverable

- **GIVEN** 对局结算过程中发生数据库或 flush 错误
- **WHEN** 系统无法完整提交双方状态
- **THEN** 系统 MUST 记录可定位的失败上下文，包括 game id、winner、loser 和失败阶段
- **AND** 系统 MUST 保留可重试或人工补偿依据

### Requirement: Asset ledger records balance-changing operations

系统 MUST 为影响玩家资产余额的操作记录资产流水或等价审计记录，至少包含玩家 id、道具 id、数量变化、来源类型、幂等键、发生时间和关联业务 id。

#### Scenario: Reward creates ledger entry

- **GIVEN** 玩家通过任务获得 `coin`
- **WHEN** 系统增加玩家 `coin` 数量
- **THEN** 系统 MUST 记录一条资产流水
- **AND** 流水 MUST 能关联到任务奖励幂等键

#### Scenario: Purchase creates debit and credit entries

- **GIVEN** 玩家购买道具并消耗货币
- **WHEN** 购买成功提交
- **THEN** 系统 MUST 记录货币扣减流水
- **AND** 系统 MUST 记录商品增加流水
- **AND** 两条流水 MUST 能关联到同一次购买操作

### Requirement: Manual verification covers consistency and recovery

实现完成后 MUST 提供手动验证步骤，覆盖正常路径、并发路径、失败重试路径和服务关闭刷盘路径。验证 MUST 包含数据库查询或日志检查，不能只依赖 HTTP 200 或 WebSocket 消息。

#### Scenario: Verification confirms no partial reward

- **GIVEN** 测试环境中触发任务奖励或邀请奖励
- **WHEN** 人为制造重试或重复请求
- **THEN** 验证步骤 MUST 检查背包数量、资产流水和幂等记录
- **AND** 结果 MUST 证明奖励没有重复或丢失

#### Scenario: Verification confirms dirty flush recovery

- **GIVEN** 玩家聚合存在 dirty 分桶
- **WHEN** 测试触发 flush、失败重试或服务关闭刷盘
- **THEN** 验证步骤 MUST 检查数据库最终状态与内存修改一致
- **AND** 日志 MUST 能定位 flush 成功或失败原因
