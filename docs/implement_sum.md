实现总结
1. Player 中间缓存层 (src/database/users.rs)
   CachedEntry 结构体缓存用户数据（不含 playing addr）
   DashMap<UserId, CachedEntry> + 60秒 TTL（基于 chrono::Utc::now()）
   get(id, use_cache, friendships) 统一接口
   update() / insert() 写 PG 后自动回写缓存；invalidate_cache() 显式失效
2. 物品系统
   迁移 002_items_inventory_shop.sql：items、user_inventory、shop_items 表 + 种子数据（coin/gem/ticket）
   src/items/mod.rs：静态 ItemDef 注册表
   src/database/items.rs：add_item、consume_item（原子扣减）、purchase（事务购买）、get_inventory、get_shop
   API：GET /api/inventory、GET/POST /api/inventory/shop/{id}/buy/{item}
3. 通用任务系统
   迁移 003_quests.sql：quests、user_quest_progress（含函数式唯一索引处理 NULL 日期）、achievement_tiers、user_achievement_progress
   src/quests/mod.rs：GameEvent::GameWon / GamePlayed
   src/database/quests.rs：on_event() 统一处理主线/每日/成就进度，自动发放奖励并升级成就 tier
   游戏结束事件：在 user_mgr.rs 的 PlayedGame 处理中调用，自动发放物品奖励
   API：GET /api/quests/story、/daily、/achievements
4. 排行榜 (src/database/leaderboard.rs)
   get_top_by_rating、get_top_by_wins（支持分页）
   get_user_rank（单用户排名查询）
   API：GET /api/leaderboard?type=skill_rating|wins&page=N、GET /api/leaderboard/me
5. 好友邀请 (src/database/invites.rs)
   迁移 004_invites.sql：invite_codes + invite_code_uses 表
   create、redeem（检查过期/限额/重复使用）、list_by_creator
   API：POST /api/invites（创建）、POST /api/invites/redeem（兑换 + 自动发放奖励）、GET /api/invites（我的邀请码）