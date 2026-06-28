## ADDED Requirements

### Requirement: Directed push to a specific player

系统 MUST 支持向指定玩家定向推送服务端消息。目标在线时，消息 MUST 通过其绑定连接经可靠层投递；目标离线时，系统 MUST NOT 阻塞调用方，且本期 MUST NOT 要求离线消息持久化（离线消息可丢弃，由客户端下次上线自行拉取最新状态）。推送消息 MUST 使用 JSON 子协议，不影响现有四子棋文本消息。

#### Scenario: Push to online player

- **GIVEN** 目标玩家在线且已绑定连接
- **WHEN** 系统向该玩家定向推送一条消息
- **THEN** 系统 MUST 通过其连接经可靠层投递该消息

#### Scenario: Push to offline player does not block

- **GIVEN** 目标玩家不在线
- **WHEN** 系统尝试向该玩家推送
- **THEN** 系统 MUST NOT 阻塞调用方
- **AND** 系统 MAY 丢弃该消息而不报错

### Requirement: Leaderboard update push

系统 MUST 在对局结算导致 SR（`skill_rating`）变化时，向当前榜单 Top N 中的在线玩家推送排行榜更新通知。推送 MUST 在结算事务提交后触发（不阻塞结算流程），目标集合 MUST 为按 `skill_rating` 取 Top N 后过滤出的在线玩家；离线的 Top N 玩家 MUST 被安全跳过。

#### Scenario: Push leaderboard update to online top-N players

- **GIVEN** 一场对局结算使参与者 SR 发生变化
- **WHEN** 系统在结算提交后触发排行榜更新推送
- **THEN** 系统 MUST 查询当前 Top N 榜单并向其中在线的玩家推送排行榜更新消息
- **AND** 系统 MUST NOT 阻塞结算流程，且 MUST 安全跳过离线的 Top N 玩家

### Requirement: Friend online push

系统 MUST 在某玩家上线时，向其在线好友推送该玩家上线的通知。

#### Scenario: Notify online friends when a player comes online

- **GIVEN** 玩家 A 登录并被判定在线
- **WHEN** 系统处理 A 的上线
- **THEN** 系统 MUST 向 A 的每个在线好友推送 A 上线的通知
- **AND** 系统 MUST NOT 向离线好友阻塞式投递

### Requirement: Achievement unlocked push

系统 MUST 在玩家达成成就时，向该玩家推送成就达成通知。

#### Scenario: Push achievement unlocked to the player

- **GIVEN** 玩家在线且触发某成就达成
- **WHEN** 系统确认该成就达成
- **THEN** 系统 MUST 向该玩家推送成就达成消息
