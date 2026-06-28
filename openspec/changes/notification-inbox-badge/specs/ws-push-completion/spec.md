# Spec: WS Push Completion

## 现有推送（已实现）

| 消息类型 | 触发点 | 目标用户 |
|---------|--------|---------|
| `AchievementUnlocked { achievement_id, tier }` | game settlement（GameWon） + complete_level HTTP | 当事人 |
| `FriendOnline { user_id }` | WS 连接建立后登录 | 在线好友 |
| `LeaderboardUpdate { top }` | game settlement 后 | 排名前 20 在线用户 |

## 新增推送

### quest_completed
```
GP:{"kind":"quest_completed","quest_id":"story_buy_shop_item","quest_type":"story"}
```
- **触发**：`apply_quest_event_in_tx` 内任意 story/daily quest `completed_at` 首次被设置时
- **目标**：当事用户（如在线）
- **客户端动作**：刷新 quests 模块，显示"可领取"徽章

### progressive_milestone
```
GP:{"kind":"progressive_milestone","achievement_id":"journey_level_milestone","step":1}
```
- **触发**：`check_progressive_achievements_in_tx` 有里程碑被跨越时（每个里程碑一条）
- **目标**：当事用户（如在线）
- **客户端动作**：刷新成就模块；新消息提示进入 Inbox

### new_inbox_message
```
GP:{"kind":"new_inbox_message","id":42,"type":"progressive_achievement","has_reward":true}
```
- **触发**：写入 `user_inbox` 后
- **目标**：当事用户（如在线）
- **客户端动作**：显示站内信弹窗 or Inbox 红点

## 实现要点

- `GameMsgOut` 新增三个变体（`src/game/msg.rs`）
- `apply_quest_event_in_tx` 返回已完成的 quest_id 列表，由调用方（`complete_level`、`settlement`、`purchase`）负责推送
- `check_progressive_achievements_in_tx` 返回 milestone step 列表，由调用方推送
- 推送失败（用户不在线）静默忽略，不影响主流程
