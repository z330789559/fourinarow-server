# minigame-anonymous-auth Specification

## Purpose
TBD - created by archiving change minigame-anonymous-auth. Update Purpose after archive.
## Requirements
### Requirement: 匿名账号注册端点

系统 SHALL 提供 `POST /api/auth/anonymous`，接受客户端设备 ID，返回可用的账号标识、会话 token 与自动生成的昵称，无需用户名或密码。

#### Scenario: 首次匿名注册

- **WHEN** 客户端以合法 `device_id` POST `/api/auth/anonymous`，且该 device_id 从未注册
- **THEN** 系统 MUST 新建一个用户（provider=`anon`，password 为空）
- **AND** MUST 新建一条 session 并返回 `{ user_id, session_token, username }`
- **AND** `username` MUST 形如「英歌侠####」且全表唯一

#### Scenario: 同设备幂等

- **WHEN** 客户端用**已注册过**的同一 `device_id` 再次调用
- **THEN** 系统 MUST 返回与首次相同的 `user_id`
- **AND** MUST NOT 修改该用户已有的 `username`
- **AND** MUST 返回一个可用的 `session_token`

#### Scenario: 非法 device_id

- **WHEN** `device_id` 缺失、为空或超过长度上限
- **THEN** 系统 MUST 返回 400
- **AND** MUST NOT 创建任何用户或 session

### Requirement: 复用既有账号与会话基建

匿名账号 SHALL 与平台账号同构，使其 session token 可用于既有 SessionToken 鉴权接口，并为未来绑定平台预留身份行。

#### Scenario: 会话可用于既有接口

- **WHEN** 用匿名注册返回的 `session_token` 作为 `SessionToken` 头调用任一需要鉴权的既有接口（如 `GET /api/users/me`）
- **THEN** 系统 MUST 正常解析出该匿名用户

#### Scenario: 预留平台绑定锚点

- **WHEN** 匿名账号被创建
- **THEN** 系统 MUST 在 `auth_identities` 写入一行 `provider='anon', provider_user_id=device_id`
- **AND** 该用户 MUST 能在未来追加其他 provider 行而不冲突

