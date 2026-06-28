## Why

当前系统在 WS 连接下只推送了成就解锁、好友上线、排行榜变动三类事件，任务完成、渐进成就达成均无通知。HTTP 模式下更是完全缺少通知机制：玩家完成关卡后无法感知任务进度变化，只能靠刷新页面。

此外，目前渐进成就奖励以"直接发放到背包"的方式处理，没有"收到通知→点击领取"的完整流程，缺少仪式感，也无法承载结算结果、系统发放等运营场景。

本 change 构建三层通知体系：WS 实时事件推送（补全缺失推送）、红点/Badge 持久化系统、站内信 Inbox（含可领取奖励）。同时把渐进成就奖励从"直接发放"改为"写入 Inbox 等待领取"，统一奖励入口。

## What Changes

- **WS 事件推送补全**：新增 `quest_completed`、`progressive_milestone`、`new_inbox_message` 三种推送消息类型。
- **红点系统**：新建 `user_notification_badges` 表，按模块（quests / achievements / friends / inbox）维护红点状态；提供查询与清除 API。
- **站内信 Inbox**：新建 `user_inbox` 表，支持多类型消息（quest_reward / achievement_reward / progressive_achievement / battle_settlement / system_gift）；提供列表、领取、删除 API。
- **渐进成就奖励改为 Inbox**：`check_progressive_achievements_in_tx` 不再直接发放背包，改为写入 `user_inbox`，同时设置 inbox 红点。
- **任务完成 WS 推送**：`apply_quest_event_in_tx` 完成后，若任务 `completed_at` 刚设置，通过 `connection_mgr` 推送 `quest_completed`。

## Capabilities

### New Capabilities

- `ws-push-completion`: 补全 WS 推送——任务完成（quest_completed）、渐进成就里程碑（progressive_milestone）、站内信新消息（new_inbox_message）。
- `notification-badge`: `user_notification_badges` 表，按模块维护红点；`GET /api/notifications/badges`、`POST /api/notifications/badges/{module}/clear`。
- `inbox`: `user_inbox` 表，完整 Inbox CRUD；`GET /api/inbox`、`POST /api/inbox/{id}/claim`、`DELETE /api/inbox/{id}`；奖励领取原子事务。

### Modified Capabilities

- `game-progression`（来自 realtime-and-progression）：`check_progressive_achievements_in_tx` 写 Inbox 代替直接发放背包。

## Out of Scope

- 邮件推送（Email / 微信模版消息）：本期只做站内
- Inbox 分页（初期数量少，一次全量）
- 消息过期自动清理（expires_at 预留字段，清理 cron 不在本期）
- 系统广播消息（to_all）
