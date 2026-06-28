## Why

英歌小游戏（game_key `yingge`）客户端目前没有任何服务端身份：进度、分数都只存在本地 localStorage，无法上报、无法上榜、换设备/换会话即丢失。现有账号体系要求用户名+密码（`/api/users/register`）或平台授权（`/api/platform/{wechat,douyin}/login`），对"打开即玩"的小游戏体验过重。

需要一个**免注册的匿名身份**：客户端用设备 ID 一次性换取账号与 session token，同设备复调返回同账号（设备绑定持久），并为以后升级绑定微信/抖音预留路径。

## What Changes

- 新增端点 `POST /api/auth/anonymous`，body `{ device_id }` → `{ user_id, session_token, username }`。
- 复用现有 `UserCollection::find_or_create_platform_user` 的免密建号模式，新增 provider `"anon"`、`provider_user_id = device_id`。
- 服务端自动生成唯一昵称，形如「英歌侠####」（4 位数字，碰撞重试/追加后缀）。
- 新增 `api::auth` 模块并在 `api::config` 挂载 `/auth` scope。

## Capabilities

### New Capabilities

- `minigame-anonymous-auth`: 设备绑定的匿名账号注册端点；返回 user_id + session_token + 自动生成昵称；同设备幂等返回同账号。

## Out of Scope

- 匿名账号→微信/抖音的实际绑定/合并流程（仅在 `auth_identities` 表上预留 provider 行，本期不做迁移合并）。
- 现有密码注册/登录与平台登录的任何改动。
- 限流/防滥用强校验（仅做基础参数校验，反滥用后续单独治理）。
- 能量、金币等经济数据上云（保持客户端）。
