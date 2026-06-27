## Context

现有 WS 架构：
- 文本协议跑在自研可靠层之上（`ACK::id`、`MSG::id::content`、`HELLO::ver::...`），见 `src/game/msg.rs`。`PlayerMessage::parse` 对整串做 `to_uppercase()`，**不适合承载 JSON / 大小写敏感数据**；可靠层 `ReliablePacketIn::parse` 用 `orig.split("::")` 拆包并要求 `MSG` 段恰好 3 段。
- `ConnectionManager` 以 `WSSessionToken`（32 位随机串）为键维护 `connections`，**没有 `user_id → 连接` 反向映射**。已存在 `CONNECTION_KEEPALIVE_SECONDS = 30` 与每秒一次的超时检测：`ConnectionState::Disconnected(Instant)` 超过 30s 即移除连接。
- 已有全局广播（chat 遍历所有连接）与 `CurrentServerState` 推送，可作为推送实现的参考。
- 账号关联已存在：`PlayerMessage::Login(SessionToken)`，登录在 `ClientState` 处理。
- `src/logging/game_logging.rs` 仅有 `GameLogEvent::{StartGame, EndGame}`，无通用行动日志。
- `player-cache-management` 已提供 `PlayerRepository` 与玩家域写入口、`asset_ledger`、幂等表（`player_operation_idempotency`）、`settle_game`/`purchase` 等同步事务范式，可复用。
- `leaderboard` 为实时查询（`get_top_by_rating`/`get_top_by_wins`，`SELECT ... ORDER BY skill_rating DESC`），无物化榜单；SR 变化点在对局结算 `settle_game`。
- 配置文件已就位；下一个 migration 序号为 `006`。

## Goals / Non-Goals

**Goals**
- WS 在 HTTP 登录后建立并保活，服务端可判断玩家在线/离线。
- 提供面向指定 user 的服务端推送。
- 统一玩家行动日志并批量落库。
- 加载模式/关卡配置，落地玩家进度与开始/完成关卡协议。

**Non-Goals（本期不做）**
- 场景地图（`MinigameSceneConfigData.json`）服务端承载、地图状态同步、全局 tick。
- 改写现有四子棋文本协议（仅做 `splitn` 健壮性修复，不改其语义）。
- 背包/商店/任务从 HTTP 迁移到 WS。

## Decisions

### Decision 1: WS 登录即连 + `user_id ↔ 连接` 映射

复用现有 `LOGIN(SessionToken)`：`ClientState` 登录成功后通知 `ConnectionManager` 建立 `user_id → WSSessionToken` 反向索引。一账号一连接（复用现有 `CloseOtherClientLogin`）：同账号新连接登录时关闭旧连接并更新映射。心跳沿用 `CONNECTION_KEEPALIVE_SECONDS = 30`。

备选：单独的 PresenceManager actor。否决理由——在线状态与连接生命周期强绑定，直接放在 `ConnectionManager` 内聚更低成本。

**Decision 1 Presence Bind 精化（M4-presence）**：在线状态以 `ConnectionManager` 的 `user_id → session_token` 映射为准，`UserManager.playing` 保留现有对战/邀请语义不变。为解决踢出路径的并发竞态，新增两条 actor 消息代替直接操作映射：

- `BindUser { user_id: UserId, session_token: WSSessionToken, state_addr: Addr<ClientState> }`：新连接登录成功后由 `ClientState` 发给 `ConnectionManager`；actor turn 内先安装新映射，再关闭旧 token 对应的连接（向旧 `ClientState` 发 `CloseOtherClientLogin`）。
- `UnbindUser { user_id: UserId, session_token: WSSessionToken, reason: UnbindReason }`：登出或连接被踢出时发送；只有当前映射仍指向该 `session_token` 才执行清理（token 不匹配则静默忽略），防止旧连接误清新连接映射。

踢出路径顺序（**MUST 遵守**）：
1. 新连接 `StartPlaying` 成功 → `ClientState` 发 `BindUser` → `ConnectionManager` 同一 turn 内安装新映射并关闭旧连接。
2. 旧连接随后 `StopPlaying` / `Disconnect` 触发 `UnbindUser(old_token)` → token 不匹配 → 只清旧 `connections` entry，不影响新连接映射。
3. 结果：最多有极短"旧连接正在关闭"窗口，不会出现"新连接被旧连接误清"的全离线，也不会出现 presence 双在线。

### Decision 2: 在线/离线判定与离线日志

区分两态：**短暂断线**（`Disconnected`，30s 内可重连，不算离线）与 **判定离线**（超时被移除）。在现有超时移除点（`check_connectionstate_interval`），若被移除连接已登录（能查到 `user_id`），发出 `ActivityEvent::Offline`。登出（`Logout`）也清理映射但单独记 `Logout` 事件。

### Decision 3: 新游戏消息采用 JSON 子协议（不改旧文本协议语义）

新增消息在可靠层 `MSG::id::content` 的 `content` 内用 JSON：约定前缀 `G:`（上行）/ `GP:`（下行推送）标识“游戏 JSON 消息”。`PlayerMessage` 新增 `GameProtocol(GameMsgIn)`，`ServerMessage` 新增 `GameProtocol(GameMsgOut)`，`GameMsgIn/Out` 用 `serde_json`。现有四子棋文本分支完全保留。

**分流落点（B1）**：`PlayerMessage::parse` 入口必须对 **原始 `orig`（而非 `to_uppercase()` 后的 `s`）** 检测前缀；命中 `G:` 则走 `serde_json` 解析并直接 `return`，**不执行后续 `to_uppercase()` 与文本匹配逻辑**。`client_adapter.rs` 的消息分发（`received_reliable_pkt` → 转发）必须新增 `PlayerMessage::GameProtocol` 分支转发到游戏 handler；`ServerMessage::GameProtocol` 序列化为 `GP:` + JSON，经可靠层 `MSG::id::content` 下行，且序列化路径 **不得经过 `to_uppercase()`**。

**可靠层分隔符（P1-1）**：可靠层 `ReliablePacketIn::parse` 现用 `orig.split("::")` 且要求 `MSG` 段 `parts.len() == 3`（msg.rs:22/29），JSON content 内一旦出现 `::` 会被拆坏成 `InvalidContent`。**MUST 改为 `orig.splitn(3, "::")`**，使 `MSG::id::content` 的 content（`parts[2]`）完整保留（含 `::`）。该改动兼容 `ACK::id`（`splitn` 后 len=2）与现有文本 content（无 `::`），但属可靠层核心，须回归验证四子棋协议不回归。

**`HELLO` 的 `protocol_version` 决议：本期不 bump**。JSON 子协议仅作用于登录后长连接；客户端契约规定 **对未知前缀消息必须静默忽略**，保证旧客户端向后兼容。

**Decision 3 splitn 回归测试要求（M4-splitn）**：现有四子棋文本 content（`PC:3`、`LOGIN:token`、`BATTLE_REQ:id`）与聊天 content（标准 base64，字符集无 `:`）内均无合法 `::`，`splitn(3, "::")` 对合法消息行为不变。但 **MUST** 在改动同 PR 内补充以下 4 条回归测试（单元测试即可），固定语义边界：
1. `MSG::1::PC:3` → 可靠层正常解包，进入文本分支，解析为 `PlayerMessage::PlacePiece(3)`。
2. `MSG::1::CHAT_MSG:<base64>` → 可靠层正常解包，进入文本分支，聊天内容完整。
3. `MSG::1::G:{"kind":"start_game"}` → 可靠层 `parts[2]` 完整为 `G:{"kind":"start_game"}`，进入 JSON 分支，解析为 `GameMsgIn::StartGame`。
4. `MSG::1::G:{"kind":"x","data":"a::b"}` → `parts[2]` 为 `G:{"kind":"x","data":"a::b"}`（`::` 在 JSON 内部被保留），JSON 解析正常，content 不被截断。
   （原 `split` 下此 case 会拆成 4 段并返回 `InvalidContent`；`splitn(3)` 后正确通过。）

### Decision 4: 行动日志 = 事件抽象 + 批量 flush worker

新增 `ActivityLogger`（参考现有 `Logger` actor）：
- 统一事件枚举 `ActivityEvent`（`Register/Login/Logout/Offline/Purchase/UseItem/CompleteQuest/ClaimReward/AddFriend/CompleteLevel/...`），含 `user_id`、`action_type`、`detail`、`created_at`。
- 内部 `Vec<ActivityEvent>` 缓冲；**满 100 条立即 flush，或 tokio interval 每 5s flush**（先到者触发），批量 `INSERT`。
- ~~业务侧 `do_send`（fire-and-forget）~~ **→ 已更正（见 Decision 4 Mailbox 精化）**：业务侧通过 `ActivityLogHandle::record(event)` 调用，内部用 `try_send`；actor `started` 时 `ctx.set_mailbox_capacity(8192)`；满队则丢弃 + drop counter 计数 + 限频 warn；不暴露裸 `Addr` 给业务侧。
- 失败有限次重试后丢弃并告警（日志非资产，弱一致）。
- **优雅停机兜底（m2）**：`main.rs` 在 `HttpServer` 停止前对 `ActivityLogger` 触发一次强制 flush。

**Decision 4 Mailbox 精化（M4-mailbox）**：Actix 0.13.5 的 `do_send` 绕过 mailbox capacity 直接入队，会无限堆积内存，`ctx.set_mailbox_capacity()` 对 `do_send` 无效。因此：
- **MUST** 将对外接口改为 `ActivityLogHandle` wrapper struct，内部调用 `Addr::try_send`。
- `ActivityLogger::started` 里 `ctx.set_mailbox_capacity(8192)`（可根据压测调整，上限约 16384）。
- `try_send` 返回 `Err(SendError::Full)` 时递增 `drop_counter`；每 `WARN_EVERY`（如 100）次 drop 打印一次 warn。
- **不向外暴露 `Addr<ActivityLogger>`**，防止业务侧绕过 handle 直接 `do_send`。

新表 `activity_log(id BIGSERIAL, user_id VARCHAR(12) NULL, action_type VARCHAR(50), detail JSONB NULL, created_at TIMESTAMPTZ)`，`user_id` 允许空以容纳注册前/匿名事件；建索引 `(user_id, created_at DESC)` 与 `(action_type, created_at)`。

### Decision 5: 玩家分模式进度存储与初始化

新表 `player_mode_progress(user_id VARCHAR(12) REFERENCES users(id) ON DELETE CASCADE, mode INT, level INT NOT NULL DEFAULT 0, updated_at TIMESTAMPTZ, PRIMARY KEY (user_id, mode))`，其余字段（如星数汇总）**预留**。`level` 语义 = 玩家在该模式 **已完成的最高关卡 `id`**（`0` = 未完成任何关卡）。

**注册初始化分层（M2）决议**：**不在 `database/users.rs` 跨层调用 `PlayerRepository`**。`database/users.rs` 仅负责 `users`/`auth_identities` 插入；由 **业务层（`UserManager` 的 `Register` handler / `src/api/users`）在注册成功后** 调用 `PlayerRepository::init_mode_progress(user_id, &modes)`（每个 mode `INSERT ... ON CONFLICT (user_id, mode) DO NOTHING`）。

**初始化非原子兜底（P1-3）**：用户创建与进度初始化非同一事务，若初始化失败用户可能已建号并拿到 session。为保证“进入游戏前每模式有进度行”的不变量，系统 MUST 在 `StartGame` / `complete_level` 入口对缺失模式 **懒补齐**（`ensure_mode_progress`，`ON CONFLICT DO NOTHING`），亦兼容后续模式集合扩展。

模式解锁条件：玩家在征途（mode 1）的 `level >= ModeConfig.unlockByJourneyLevel`。mode1/mode4 的 `unlockByJourneyLevel=1`（始终可选），mode2=60、mode3=120。

### Decision 6: 配置启动加载（内存只读）

启动时加载 `MinigameModeConfigData.json` 与 `MinigameLevelConfigData.json` 到进程内只读结构（`once_cell`/`Arc` 注入 app data），按 `mode` 建立索引、按关卡 **`id`** 在 mode 内升序排序以支持游标分页、统计每模式关卡总数。**fail-fast**：缺失/解析失败则启动失败。**不加载** `MinigameSceneConfigData.json`。

**关卡身份以 `(mode, id)` 标识（P0-1 更正）**：经核对配置，同一 mode 内 `levelMin` 仅在 `1..10` 循环（非唯一，mode1 的 245 关重复约 24 轮），`id` 才是 mode 内连续唯一序号；`levelMin/levelMax/chapter` 仅作卡片展示属性，**MUST NOT** 用作分页游标或进度序号。

### Decision 7: 开始游戏 / 翻页 / 完成关卡协议

- 关卡身份统一以 `(mode, id)` 标识；玩家进度 `level` = 该 mode 已完成的最高关卡 `id`，“当前可完成关卡” = `level + 1`。
- `GameStartReq` → `GameStartResp { unlocked_modes, locked_modes, selectable_levels, level_count_per_mode }`；`selectable_levels` 为各可选模式 `level + 1` 对应的关卡。
- **翻页（P0-1 更正）**：`LevelPageReq { mode, after_id, page_size }` → `LevelPageResp { cards }`，返回该 mode 下 `id > after_id` 的下一批关卡卡片（按 `id` 升序）；卡片携带 `levelMin/levelMax/chapter/ammoCount/星数阈值/sceneId` 等展示属性。
- **完成关卡（P0-2 / P1-2 / P2-1）**：`LevelCompleteReq { mode, level_id, stars }` → 校验：① 模式已解锁；② `(mode, level_id)` 属于该模式且 `level_id == current_level + 1`（顺序闯关，拒绝跳级）；③ 星数仅校验 `∈ [1,3]`（指标级判定延后）。校验通过 → 经 `PlayerRepository::complete_level` **参照 `settle_game` 同步事务范式**：flush 缓存 → `begin` → 幂等键 `level_complete:{user_id}:{mode}:{level_id}` 写 `player_operation_idempotency`（命中则 rollback 幂等返回）→ `UPDATE player_mode_progress SET level = level_id` → `asset_ledger` 发奖 → `commit` → 提交后刷新缓存。`CompleteLevel` 行动日志与成就推送在 **事务提交后异步触发**（`do_send`，弱一致，不入事务）→ `LevelCompleteResp { ok, new_level, rewards }`。本期 `player_mode_progress` 仅存 `level`，不存每关星数。
- **进度读写与缓存（P2-1）**：`PlayerAggregate` 增加 `mode_progress`（只读载入）；进度的 **写一律走同步事务**（`complete_level` / `init_mode_progress`），**不引入 `DirtyBucket::ModeProgress` write-behind**，避免与奖励/幂等/资产流水的强一致冲突。

### Decision 8: 排行榜更新推送（Top N 在线）

触发点 = 对局结算 `settle_game` 使 SR 变化。**在结算事务提交后的上层调用处**（非 repository 内、不阻塞结算）：查询当前 `get_top_by_rating(N)` → 用 `is_online` 过滤出在线者 → 经 `PushToUser` 推送排行榜更新。`N` 为配置项。频率/节流策略列为 Deferred（默认变化即推 + 简单去重）。

## Risks / Trade-offs

- **WS 协议扩展**：必须同步 `client_adapter` 可靠层与客户端契约；`splitn` 改动触及可靠层核心，须回归四子棋协议。
- **JSON 与 `to_uppercase()` / `::` 冲突**：新 JSON 分支必须独立解析路径且可靠层改 `splitn(3)`，否则 payload 被破坏。
- **进度与日志一致性边界**：完成关卡的进度/奖励/幂等/资产走 **同步事务强一致**；行动日志与推送为 **提交后异步弱一致**，二者边界须清晰，严禁把行动日志写进强一致事务。
- **进度缓存语义**：`mode_progress` 只读入缓存、写走同步事务，避免被误当作 write-behind 脏桶。
- **在线映射并发与多端登录**：`UserManager.user.playing` 与 `ConnectionManager` 反向映射两套机制须在登录/登出/踢出时同步。
- **完成关卡防作弊**：本期以“顺序闯关 + 星数范围校验”为底线，指标级判定延后。

## Resolved Decisions（原 Open Questions / 审查项，本期已收敛）

- **关卡身份与 levelMin 语义（P0-1 更正）**：关卡身份、进度序号、分页一律用 `(mode, id)`；`levelMin` 仅 1–10 循环、非唯一，仅作展示。详见 Decision 6 / 7。
- **完成关卡校验**：`level_id == current_level + 1`（以 `(mode, id)` 为身份），顺序闯关、拒绝跳级。详见 Decision 7。
- **星数校验（P0-2）**：本期仅范围校验 `∈ [1,3]`；spec 同步改为范围校验，指标级判定延后。
- **星数存储**：本期不存每关星数，`player_mode_progress` 仅 `level`。
- **注册初始化（M2 / P1-3）**：业务层注册后 best-effort 初始化 + `StartGame`/`complete_level` 懒补齐兜底。
- **完成关卡事务与日志（P1-2 / P2-1）**：进度/奖励/幂等走同步事务（参照 `settle_game`）；行动日志与推送提交后异步；进度不走 `DirtyBucket` write-behind。
- **排行榜推送目标（Decision 8）**：对局结算 SR 变化后，向当前 Top N 榜单中的在线玩家推送（提交后异步，不阻塞）。
- **`HELLO` protocol_version**：本期不 bump，客户端对未知前缀消息静默忽略。
- **ActivityLogger mailbox 策略（M4-mailbox）**：对外暴露 `ActivityLogHandle` wrapper，内部 `try_send`；actor started 时 `set_mailbox_capacity(8192)`；满队丢弃 + drop counter + 限频 warn；不暴露裸 `Addr`。详见 Decision 4 Mailbox 精化。
- **Presence BindUser/UnbindUser（M4-presence）**：在线状态以 `ConnectionManager` 的 `user_id → session_token` 为准；`BindUser`/`UnbindUser` 均携带 `session_token` 做 guard，防止旧连接误清新连接映射；踢出路径先安装新映射再关旧连接。详见 Decision 1 Presence Bind 精化。
- **splitn 回归测试（M4-splitn）**：`splitn(3, "::")` 对现有合法 content 行为不变；MUST 在同 PR 内补 4 条回归测试（text 协议 2 条 + JSON 无 `::` 1 条 + JSON 含 `::` 1 条）。详见 Decision 3 splitn 回归测试要求。

## Deferred（本期延后，实现期或后续 change 决定）

- 排行榜推送的频率/节流（目标集合已定为 Top N 在线；变化即推 vs 周期节流去重，按压测调整）。
- 行动日志 `detail` 的 JSONB 结构与保留/清理策略。
- 服务端按指标（`ammoCount`/`tripleStar` 等）的星数防作弊判定：后续单独 change。
