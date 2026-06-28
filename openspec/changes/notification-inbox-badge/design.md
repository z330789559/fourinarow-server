# Design: notification-inbox-badge

## 架构决策

### 1. 红点不走业务事务
红点写入（`upsert user_notification_badges`）在主事务提交后异步执行，丢失可接受（下次触发会再写）。如果放进事务，红点错误会回滚业务操作，得不偿失。

### 2. Inbox 奖励领取 vs 任务奖励领取
任务奖励（story/daily claim）保持原有 `POST /api/quests/{type}/{id}/claim` 不变，Inbox 有自己独立的 `POST /api/inbox/{id}/claim`。两套流程互不干扰。

渐进成就奖励迁移到 Inbox：因为渐进成就奖励需要"仪式感"（通知→查看→领取），且金额随 step 增长，适合独立入口。

### 3. WS 推送由调用方负责，不在 tx 内
`apply_quest_event_in_tx` / `check_progressive_achievements_in_tx` 只返回结果（已完成 quest、已触发 milestone），推送动作由调用方（`complete_level`、`settlement`、`purchase`）在 tx commit 后执行。原因：tx 内发 WS 消息若 rollback 则消息已发，造成幻通知。

### 4. connection_mgr 引用传递方式
HTTP handler（`gameplay.rs`、`inventory.rs` 等）已通过 `web::Data<Addr<ConnectionManager>>` 获取推送能力。`repository.rs` 中的 `apply_quest_event_in_tx` 不直接持有 connection_mgr，统一由上层 handler 推送。

### 5. apply_quest_event_in_tx 返回值扩展
当前返回 `(Vec<(String,i32)>, Vec<(String,i32)>)` = (item_rewards, unlocked_achievements)。  
新增第三个字段：`Vec<String>` = 本次调用中 completed_at 被首次设置的 quest_id 列表，供调用方推送 `quest_completed`。

## 数据流（完成关卡为例）

```
POST /api/game/complete_level
  │
  ├─ DB transaction
  │    ├─ update player_mode_progress
  │    ├─ apply_quest_event_in_tx
  │    │    └─ returns: (rewards, achievements, completed_quest_ids)
  │    ├─ check_progressive_achievements_in_tx
  │    │    ├─ INSERT user_inbox (奖励不直接发放)
  │    │    └─ returns: milestones_triggered: Vec<(achievement_id, step)>
  │    └─ COMMIT
  │
  ├─ 写红点（独立，post-commit）
  │    ├─ quests badge (if completed_quest_ids non-empty)
  │    └─ inbox badge (if milestones_triggered non-empty)
  │
  └─ WS 推送（post-commit，用户在线才有）
       ├─ quest_completed × completed_quest_ids.len()
       ├─ progressive_milestone × milestones_triggered.len()
       └─ new_inbox_message × inbox_ids written
```

## Migration

**008_notification_inbox_badge.sql**
- `user_notification_badges`
- `user_inbox`

## 新增文件

- `src/api/notifications.rs` — badges 接口
- `src/api/inbox.rs` — inbox 接口
- `src/database/notifications.rs` — badge upsert/query helpers

## 修改文件

- `src/game/msg.rs` — 新增 3 个 GameMsgOut 变体
- `src/player/repository.rs` — apply_quest_event_in_tx 返回值 + check_progressive 改写 inbox
- `src/api/gameplay.rs` — post-commit 推送
- `src/api/users/user_mgr.rs` — settlement post-commit 推送
- `src/api/mod.rs` — 注册新路由
