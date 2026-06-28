## ADDED Requirements

### Requirement: WS connection established and bound to account after login

系统 MUST 支持玩家在完成 HTTP 登录后建立 WebSocket 长连接，并通过现有 `LOGIN` 消息将连接与账号绑定。系统 MUST 维护 `user_id → 连接` 的映射，以支持按玩家定向投递消息与判断在线状态。映射 MUST NOT 改变现有四子棋对战连接流程。

#### Scenario: Bind connection on login

- **GIVEN** 玩家已完成 HTTP 登录并持有有效 session token
- **WHEN** 客户端建立 WS 并发送 `LOGIN`
- **THEN** 系统 MUST 将该连接与对应 `user_id` 绑定
- **AND** 系统 MUST 在 `user_id → 连接` 映射中记录该连接

#### Scenario: One active connection per account

- **GIVEN** 某账号已有一个绑定的活跃连接
- **WHEN** 同一账号在新连接上登录
- **THEN** 系统 MUST 关闭旧连接
- **AND** 系统 MUST 将映射更新为指向新连接

### Requirement: Keepalive heartbeat

系统 MUST 通过心跳维持 WS 长连接，心跳/保活窗口 SHOULD 为 30 秒。在保活窗口内未收到心跳或数据的连接 MUST 被视为断线候选。

#### Scenario: Connection kept alive by heartbeat

- **GIVEN** 一个已建立的 WS 连接
- **WHEN** 客户端在保活窗口内持续发送心跳或消息
- **THEN** 系统 MUST 保持该连接为在线状态
- **AND** 系统 MUST NOT 将其判定为离线

### Requirement: Online/offline determination and offline logging

系统 MUST 区分“短暂断线（可在保活窗口内重连）”与“判定离线（超时被移除）”。当已登录连接因超时被移除时，系统 MUST 判定该玩家离线、从映射中移除，并产生一条离线行动事件供 `activity-log` 记录。

#### Scenario: Reconnect within window stays online

- **GIVEN** 玩家连接短暂断开但在保活窗口（30s）内重连
- **WHEN** 重连成功
- **THEN** 系统 MUST 保持该玩家为在线
- **AND** 系统 MUST NOT 产生离线行动事件

#### Scenario: Timeout marks offline and logs

- **GIVEN** 一个已登录连接断开且超过保活窗口未重连
- **WHEN** 系统在超时检测中移除该连接
- **THEN** 系统 MUST 判定该玩家离线
- **AND** 系统 MUST 从 `user_id → 连接` 映射中移除该玩家
- **AND** 系统 MUST 产生一条离线行动事件

#### Scenario: Query online status

- **GIVEN** 系统维护着在线映射
- **WHEN** 业务侧查询某 `user_id` 是否在线
- **THEN** 系统 MUST 依据映射返回在线/离线判断
