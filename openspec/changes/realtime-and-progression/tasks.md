> 前置说明：design.md 末尾 Resolved Decisions 已收敛全部 Open Questions 与审查项（关卡身份 `(mode, id)`、星数范围校验、注册懒补齐、完成关卡同步事务、行动日志提交后异步、排行榜 Top N 在线推送、protocol_version 不 bump）。实现须对照该决议。

## 1. Schema And Config Loading

- [x] 1.1 新增 migration `006_realtime_and_progression.sql`：`activity_log`（`id BIGSERIAL`、`user_id VARCHAR(12) NULL`、`action_type VARCHAR(50)`、`detail JSONB NULL`、`created_at TIMESTAMPTZ`，索引 `(user_id, created_at DESC)` 与 `(action_type, created_at)`）与 `player_mode_progress`（`user_id VARCHAR(12) REFERENCES users(id) ON DELETE CASCADE`、`mode INT`、`level INT NOT NULL DEFAULT 0`、`updated_at TIMESTAMPTZ`，PK `(user_id, mode)`），不修改既有 `001..005`
- [ ] 1.2 用 PostgreSQL/sqlx 校验 migration 可迁移，编写最小 SQL 验证（插入/查询/唯一约束/外键级联）
- [x] 1.3 新增配置加载模块：启动读取 `MinigameModeConfigData.json` 与 `MinigameLevelConfigData.json` 到内存只读结构，**关卡身份以 `(mode, id)` 标识、按 `id` 在 mode 内升序排序以支持游标分页**、统计每模式关卡总数；fail-fast
- [x] 1.4 在 `main.rs` 启动流程注入配置，并确认**不加载** `MinigameSceneConfigData.json`

## 2. Activity Log

- [x] 2.1 定义 `ActivityEvent`（含 `user_id` 可空、`action_type`、`detail`、`created_at`）与行为类型集合
- [x] 2.2 新增 `ActivityLogger` actor：内存缓冲 + 满 100 条立即 flush + tokio interval 每 5s flush，批量 `INSERT`；`started` 时 `ctx.set_mailbox_capacity(8192)`
- [x] 2.2a 新增 `ActivityLogHandle` wrapper struct（`record(event)` → 内部 `try_send`）；满队丢弃 + `drop_counter` + 每 100 次 drop 打 warn；**不向业务侧暴露裸 `Addr<ActivityLogger>`**（见 design.md Decision 4 Mailbox 精化 M4-mailbox）
- [x] 2.3 业务侧通过 `ActivityLogHandle::record` 非阻塞接入；失败有限次重试后丢弃并告警
- [x] 2.4 接入触发点：注册、登录、登出、购买、使用物品、完成任务、领取任务奖励、添加好友（离线/完成关卡在后续阶段接入）
- [x] 2.5 优雅停机兜底（m2）：`main.rs` 在 `HttpServer` 停止前对 `ActivityLogger` 触发一次强制 flush

## 3. Presence (WS On Login)

- [x] 3.1 `ConnectionManager` 增加 `user_id → session_token` 反向映射：新增 actor 消息 `BindUser { user_id, session_token, state_addr }` / `UnbindUser { user_id, session_token, reason }`；登录成功后由 `ClientState`（`StartPlaying` 异步回调内）发 `BindUser`（见 design.md Decision 1 Presence Bind 精化 M4-presence）
- [x] 3.1a `Connection` 结构体新增 `user_id: Option<UserId>` 字段用于离线判定（超时移除时读取，见 3.4）
- [x] 3.2 一账号一连接：`BindUser` handler 同一 actor turn 内先安装新映射再关闭旧连接（`CloseOtherClientLogin`）；`UnbindUser` handler 检查 `session_token` 匹配才清理（不匹配静默忽略），防止旧连接误清新映射；**`UserManager.playing` 保留现有对战/邀请语义，在线状态以 `ConnectionManager` 映射为准（m4 风险已收敛，见 design.md M4-presence）**
- [x] 3.3 心跳保活沿用 `CONNECTION_KEEPALIVE_SECONDS = 30`，确认登录后长连接不依赖对战
- [x] 3.4 超时移除点判定离线：读取被移除 `Connection.user_id`，若 `Some` 则清理反向映射并发出 `Offline` 行动事件；登出单独记 `Logout`
- [x] 3.5 暴露 `is_online(user_id)` 查询能力供推送/业务使用

## 4. Realtime Push

- [x] 4.1 `ConnectionManager` 增加 `PushToUser(user_id, GameMsgOut)`：在线经可靠层投递，离线安全丢弃不阻塞；**将 `Addr<ConnectionManager>` 经 `web::Data` 注入到需要推送的 REST handler 与 `UserManager`，作为推送统一入口（M4）**
- [x] 4.2 好友上线推送：玩家上线时向其在线好友推送（经 4.1 注入的 Addr）
- [x] 4.3 成就达成推送：成就达成时向该玩家推送（经 4.1 注入的 Addr）
- [x] 4.4 排行榜更新推送：**在对局结算 `settle_game` 提交后的上层调用处**，查询 Top N 榜单（`get_top_by_rating`）→ 用 `is_online` 过滤 → `PushToUser`；触发不在 repository 内、不阻塞结算（Decision 8）

## 5. WS JSON Sub-protocol And Game Progression

- [x] 5.1 `msg.rs` 新增 JSON 子协议：`PlayerMessage::GameProtocol(GameMsgIn)` / `ServerMessage::GameProtocol(GameMsgOut)`；**分流在 `PlayerMessage::parse` 入口对原始 `orig`（非 `to_uppercase()` 后的 `s`）检测 `G:` 前缀，命中即 `serde_json` 解析并直接 `return`（B1）**；**并将可靠层 `ReliablePacketIn::parse` 的 `orig.split("::")` 改为 `splitn(3, "::")`，使 MSG content 完整保留 `::`（P1-1，见 design.md Decision 3 M4-splitn）**；现有文本分支不变
- [x] 5.1a 同 PR 内补 4 条 `splitn` 回归单元测试（见 design.md M4-splitn）：① `MSG::1::PC:3` 正常 → `PlacePiece(3)`；② `MSG::1::CHAT_MSG:<base64>` 正常；③ `MSG::1::G:{"kind":"start_game"}` 正常进 JSON 分支；④ `MSG::1::G:{"data":"a::b"}` 中 content 内 `::` 被完整保留
- [x] 5.2 同步 `client_adapter.rs` 可靠层与客户端契约：`received_reliable_pkt` 转发处新增 `PlayerMessage::GameProtocol` 分支转发到游戏 handler；`ServerMessage::GameProtocol` 序列化为 `GP:`+JSON 经可靠层下行且不经 `to_uppercase()`；回归验证须通过 5.1a 的单元测试；客户端契约规定未知前缀消息静默忽略
- [x] 5.3 注册时初始化每模式 `level=0` 进度：在业务层 `UserManager` 注册成功后调用 `PlayerRepository::init_mode_progress`（`ON CONFLICT (user_id, mode) DO NOTHING`），不在 `database/users.rs` 跨层（M2）；**初始化与建号非原子，失败由懒补齐兜底（见 5.8）**
- [x] 5.4 “开始游戏”协议：返回可选/不可选模式（`unlockByJourneyLevel`：mode1/4=1、mode2=60、mode3=120）、当前可选关卡（各可选模式 `level + 1` 对应关卡 `id`）、每模式关卡总数
- [x] 5.5 关卡翻页：`LevelPageReq { mode, after_id, page_size }` 返回该 mode 下 `id > after_id` 升序下一批卡片（**P0-1 更正：关卡身份/分页用 `(mode, id)`，`levelMin` 非唯一仅作展示**）
- [x] 5.6 “完成关卡”协议：经 `PlayerRepository::complete_level` **参照 `settle_game` 同步事务**——幂等键 `level_complete:{user_id}:{mode}:{level_id}` 写 `player_operation_idempotency`（命中幂等返回）→ 校验模式解锁 + `level_id == current_level + 1`（id）+ `stars ∈ [1,3]` → `UPDATE player_mode_progress SET level = level_id` → `asset_ledger` 发奖 → `commit`；**`complete_level` 行动日志与成就推送在提交后异步 `do_send`（P1-2），不入事务**；非法拒绝且不改进度、不发奖
- [x] 5.7 `PlayerRepository` 玩家域扩展（B2 / P2-1）：`PlayerAggregate` 增加 `mode_progress`（只读载入，`load_aggregate` 加 `SELECT ... FROM player_mode_progress`）；进度**写不走 `DirtyBucket` write-behind**，由 `complete_level` / `init_mode_progress` 同步事务完成、提交后刷新缓存
- [x] 5.8 懒补齐（P1-3）：`PlayerRepository::ensure_mode_progress(user_id)` helper——`StartGame` 与 `complete_level` 入口对缺失模式补齐 `level=0` 进度行（`ON CONFLICT DO NOTHING`），兜底注册初始化失败/模式集合扩展

## 6. Compatibility And Manual Verification

- [ ] 6.1 `cargo check`，确认 sqlx 查询与 `006` schema 对齐
- [ ] 6.2 手动验证四子棋对战不回归：连接、匹配、落子、`GAME_OVER` 兼容，且 `splitn(3, "::")` 不影响文本协议
- [ ] 6.3 手动验证登录后长连接 + 30s 心跳 + 断线重连不误判离线
- [ ] 6.4 手动验证离线判定写入 `activity_log`，且批量落库（5s 或 100 条）符合预期
- [ ] 6.5 手动验证推送：好友上线、成就达成、排行榜更新（目标=Top N 在线，离线不阻塞）
- [ ] 6.6 手动验证开始游戏返回结构、关卡翻页（`after_id` 游标）、完成关卡校验（`level_id == current_level+1` 合法推进 / 非法跳级拒绝 / 星数越界拒绝 / 重复幂等）
- [ ] 6.7 SQL 核验：`player_mode_progress.level` 推进（= 关卡 `id`）、`activity_log` 行为记录

## 7. Documentation And Open Questions

- [ ] 7.1 文档化新增 WS JSON 子协议消息格式（开始游戏/翻页/完成关卡/三类推送），含 `G:`/`GP:` 前缀约定、`splitn` 拆包约定与“未知前缀消息静默忽略”的向后兼容约定
- [ ] 7.2 文档化配置加载与 fail-fast、回滚开关（长连接登录注册、推送、日志 worker）
- [ ] 7.3 Open Questions 已在 design.md 收敛（见 Resolved Decisions / Deferred）；实现前复核决议一致性，Deferred 项（排行榜推送频率/节流、`detail` schema、指标级星数判定）按既有默认推进
