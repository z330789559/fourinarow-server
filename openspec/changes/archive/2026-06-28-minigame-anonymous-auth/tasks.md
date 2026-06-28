# Tasks: minigame-anonymous-auth

## 1. 匿名注册端点

- [x] 1.1 新建 `src/api/auth/mod.rs`：`AnonymousResp { user_id, session_token, username }`；请求体 `AnonymousReq { device_id }`。
- [x] 1.2 校验 `device_id`：非空、长度 ≤ 128，否则 400。
- [x] 1.3 昵称生成 helper：`英歌侠{:04}` + 复用 `make_unique_username` 处理碰撞。
- [x] 1.4 调 `db.users.find_or_create_platform_user("anon", &device_id, None, Some(&nickname), None)`；失败 500。
- [x] 1.5 回读 `db.users.get_id_public(user_id)` 取 `username`，组装 200 响应。
- [x] 1.6 `src/api/mod.rs`：`pub mod auth;` + 挂载 `web::scope("/auth").configure(auth::config)`。

## 2. 验证（无测试框架，手动）

- [x] 2.1 `cargo build` 通过。
- [x] 2.2 Postgres 就绪（容器 `pg`，PG18，库 fourinarow）；服务 `BIND=127.0.0.1:40146` 启动，migrate 011 已应用。
- [x] 2.3 curl 新建匿名账号：user_id=206bf9ad23f6 / username=英歌侠2934。
- [x] 2.4 同 device_id 复调 user_id 不变（SAME）。
- [x] 2.5 空 device_id → 400。
- [x] 2.6 session_token 经鉴权接口（`/leaderboard/me`，同 get_session_token 链路）确认会话可用。

## 3. 收尾

- [x] 3.1 验证记录见本文件「验收记录」。
- [x] 3.2 OpenSpec 归档。

## 验收记录 (2026-06-28)

curl + 客户端预览双验证（服务端 127.0.0.1:40146，库 fourinarow@容器 pg）：
- 新建匿名账号 → `{user_id, session_token, username:"英歌侠####"}`；同 device_id 复调同 user_id（幂等）；空 device_id → 400。
- session_token 经鉴权接口（`/leaderboard/me`）验证可用。
- 客户端预览实测：`POST /api/auth/anonymous` 200（Electron UA），真实玩家成功获得匿名身份（英歌侠2946）。
