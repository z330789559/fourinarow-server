## Why

当前 WebSocket 主要服务于四子棋 lobby 对战，但小游戏的实际玩法大多是“全局闯关”，真正进入 lobby/房间对战的占比很低。这导致：WS 通道利用率低、服务端无法感知玩家在线状态、缺少主动推送能力。同时玩家行为缺少统一可追踪日志；新的关卡玩法（模式/关卡/进度）也尚无服务端承载。

本 change 将 WS 从“仅对战期连接”升级为“HTTP 登录后即建立的长连接”，在其上提供在线状态判断、服务端推送、统一行动日志，并落地基于配置的关卡玩法（模式解锁、关卡选择、完成校验）。背包/商店/任务维持现有 HTTP 接口不变。

## What Changes

- **WS 长连接 + 心跳**：玩家完成 HTTP 登录后即建立 WS 并通过现有 `LOGIN` 关联账号；保持 30s 心跳（当前功能少，心跳间隔放长）。
- **在线状态**：在 `ConnectionManager` 维护 `user_id → 连接` 映射，支持判断在线/离线；连接超时判定离线并写离线行动日志。
- **服务端推送**：排行榜更新推送、好友上线推送、成就达成推送（定向到指定 user）。
- **全局行动日志**：抽象统一行动事件（注册、登录、购买、使用物品、完成任务、领取任务奖励、添加好友、完成关卡、离线等），写入日志表；采用内存队列**批量落库（每 5s 或满 100 条）**，降低写库压力。
- **关卡玩法**：
  - 启动加载模式配置（`MinigameModeConfigData.json`）与关卡配置（`MinigameLevelConfigData.json`）到内存。
  - 玩家注册时初始化每个模式的进度（`level=0`，其余字段预留）。
  - 新增 **“开始游戏”** 协议：返回可选模式、不可选模式、当前可选关卡、每个模式的关卡总数；支持按关卡 `id`（以 `(mode, id)` 为关卡身份）**翻页**获取下一批关卡卡片。
  - 新增 **“完成关卡”** 协议（WS 上行）：服务端依据玩家当前进度**校验合法性**后推进等级、发放奖励。
- **不做（本期）**：场景地图（`MinigameSceneConfigData.json`）服务端承载、地图数据状态同步、全局 tick。本期由前端保存场景数据，后续需要时再单独立项。

## Capabilities

### New Capabilities

- `presence`: WS 登录后长连接、`user_id ↔ 连接` 映射、30s 心跳、在线/离线判定与离线日志触发。
- `realtime-push`: 面向指定玩家的服务端推送（排行榜更新、好友上线、成就达成）。
- `activity-log`: 统一玩家行动事件抽象与批量落库（每 5s 或满 100 条）。
- `game-progression`: 模式/关卡配置加载、玩家关卡进度初始化、开始游戏/翻页/完成关卡协议与服务端校验。

### Modified Capabilities

- 无归档主规格。本 change 复用 `player-cache-management` 的 `PlayerRepository` 与玩家域，并在实现中扩展“玩家关卡进度”子域；不修改其既有合同。

## Impact

- **影响代码**：
  - `src/game/connection_mgr.rs`（user↔连接映射、离线判定触发日志）、`src/game/msg.rs`（新增 JSON 子协议消息）、`src/game/client_adapter.rs`（可靠层契约同步）、`src/game/client_state.rs`（登录注册映射）。
  - `src/api/users/mod.rs` + `src/database/users.rs`（注册时初始化各模式进度）。
  - `src/logging/`（新增 `ActivityLogger` 与行动事件抽象、批量 flush worker）。
  - 新增配置加载模块（启动读取 Mode/Level 配置到内存）。
  - `src/player/`（玩家关卡进度子域，复用 `PlayerRepository`）。
  - 排行榜/好友/成就相关模块接入推送触发点。
- **影响数据**：新增 migration `006_*`：`activity_log`（行动日志表）、`player_mode_progress`（玩家分模式进度表）。不修改既有 `001..005`。
- **影响协议**：WS 新增基于 JSON 的游戏子协议（开始游戏/翻页/完成关卡 + 三类推送）；现有四子棋文本协议（`PC:`、`GAME_START` 等）与可靠层 ACK/重传/重排序保持不变。
- **兼容风险**：旧客户端不感知新 JSON 子协议即可；新协议需要前端接入。WS 协议扩展须同步 `client_adapter` 可靠层与客户端契约。
- **回滚策略**：可关闭长连接登录注册（回退为对战期连接）、停用推送与行动日志 worker；新增表与配置加载可保留为空闲。不得删除已产生的行动日志。
