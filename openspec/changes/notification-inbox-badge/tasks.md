# Tasks: notification-inbox-badge

## §1 Migration 008

- [x] 1.1 新建 `migrations/008_notification_inbox_badge.sql`，含 `user_notification_badges` 和 `user_inbox` 表

## §2 WS 消息类型扩展

- [x] 2.1 `src/game/msg.rs` 新增 `QuestCompleted { quest_id: String, quest_type: String }`
- [x] 2.2 `src/game/msg.rs` 新增 `ProgressiveMilestone { achievement_id: String, step: i32 }`
- [x] 2.3 `src/game/msg.rs` 新增 `NewInboxMessage { id: i64, msg_type: String, has_reward: bool }`

## §3 红点系统

- [x] 3.1 新建 `src/database/notifications.rs`，实现 `set_badge(pool, user_id, module)` 和 `clear_badge(pool, user_id, module)` 和 `get_badges(pool, user_id) -> HashMap<module, bool>`
- [x] 3.2 新建 `src/api/notifications.rs`，实现 `GET /api/notifications/badges` 和 `POST /api/notifications/badges/{module}/clear`
- [x] 3.3 `src/api/mod.rs` 注册 `/notifications` 路由

## §4 站内信 Inbox

- [x] 4.1 新建 `src/database/inbox.rs`，实现 `insert_inbox_message`、`list_inbox`、`claim_inbox`（含 asset_ledger 事务）、`mark_read`、`delete_inbox`
- [x] 4.2 新建 `src/api/inbox.rs`，实现 `GET /api/inbox`、`POST /api/inbox/{id}/claim`、`POST /api/inbox/{id}/read`、`DELETE /api/inbox/{id}`
- [x] 4.3 `src/api/mod.rs` 注册 `/inbox` 路由

## §5 渐进成就改写 Inbox

- [x] 5.1 `src/player/repository.rs` — `check_progressive_achievements_in_tx` 改为写 `user_inbox`（不再直接写 asset_ledger + inventory），返回 `Vec<(achievement_id, step, inbox_id)>`
- [x] 5.2 `src/api/gameplay.rs` — post-commit 调 `set_badge(inbox)` + WS 推送 `progressive_milestone` + `new_inbox_message`

## §6 任务完成推送

- [x] 6.1 `src/player/repository.rs` — `apply_quest_event_in_tx` 返回值扩展为 `(item_rewards, achievements, completed_quests: Vec<(String,String)>)`（quest_id, quest_type）
- [x] 6.2 `src/api/gameplay.rs` — post-commit 根据 `completed_quests` 推送 `quest_completed` + 调 `set_badge(quests)`
- [x] 6.3 `src/api/users/user_mgr.rs` — `settlement` 和 `purchase` 路径同样处理 `completed_quests`

## §7 验证

- [x] 7.1 `cargo check` 无错误
- [ ] 7.2 手动测试：完成关卡5 → 响应无 rewards（已移入 inbox） → GET /api/inbox 可见消息 → POST /api/inbox/{id}/claim 发放到背包
- [ ] 7.3 手动测试：完成任务条件 → GET /api/notifications/badges 显示 quests:true → 调 clear → quests:false
- [ ] 7.4 WS 模式测试：完成关卡 → 收到 quest_completed / progressive_milestone 推送
- [ ] 7.5 HTTP 模式测试：无 WS，通过 GET /api/inbox 获知通知，走 claim 领奖
<!-- 服务启动: cargo run; 依赖: 迁移 008 已执行 -->
