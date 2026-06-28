# Design: minigame-anonymous-auth

## API 契约

### POST /api/auth/anonymous

**Auth**: 无（公开端点）
**Body (JSON)**:
```json
{ "device_id": "string，1..128，客户端生成的稳定 UUID" }
```
**Response 200**:
```json
{ "user_id": "string", "session_token": "string", "username": "英歌侠1234" }
```
**Response 400**: device_id 缺失/空/超长。
**Response 500**: 建号或建会话失败。

幂等：同一 `device_id` 重复调用返回**同一 user_id**（不改昵称），每次新建一条 session（与平台登录一致）。

## 实现

复用 `UserCollection::find_or_create_platform_user(provider, provider_user_id, union_id, nickname, session_key)`：
- `provider = "anon"`，`provider_user_id = device_id`，`union_id = None`，`session_key = None`。
- `nickname = Some(<生成的"英歌侠####">)`：仅首登生效（既有用户走 find 分支，不覆盖昵称）。
- 该方法已：唯一 user_id 生成、`users` 插入（`source_platform="anon"`, `password_hash=''`, `skill_rating=1000`）、`auth_identities` 插入、新建 session、返回 `(UserId, SessionToken)`。

返回体中的 `username`：成功后用既有 `db.users.get_id_public(user_id)` 回读 `username`（避免给 find_or_create 增加返回值）。

昵称生成 helper（放 `api::auth` 或 `UserCollection`）：
- base = `format!("英歌侠{:04}", rand 0..=9999)`；交给既有 `make_unique_username` 处理碰撞（追加 `_2`/随机后缀）。

模块与挂载：
- 新文件 `src/api/auth/mod.rs`，`pub fn config(cfg)` 注册 `POST /anonymous`。
- `src/api/mod.rs`：`pub mod auth;` + `.service(web::scope("/auth").configure(auth::config))`。

## 数据库

无新表、无新 migration。沿用 `users` / `auth_identities` / `sessions`。`auth_identities(provider='anon', provider_user_id=device_id)` 即设备绑定锚点，也是以后平台绑定的并存行所在。

## 兼容 / 安全 / 回滚

- 兼容：纯新增端点与新 provider 值，不动既有契约。
- 安全：`device_id` 由客户端生成，可被伪造——匿名身份本就低保障；不在此承诺防伪。不记录敏感信息。
- 回滚：移除 `/auth` scope 即可；已建匿名用户与既有平台用户同构，保留无害。

## 验证

`cargo build` 后对 `127.0.0.1:40146`：
1. `curl -s -XPOST .../api/auth/anonymous -H 'Content-Type: application/json' -d '{"device_id":"dev-test-001"}'` → 得 user_id/session_token/username。
2. 同 device_id 再调一次 → user_id 不变（username 不变）。
3. 空 device_id → 400。
