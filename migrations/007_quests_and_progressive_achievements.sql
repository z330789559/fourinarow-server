-- Migration 007: quests seed data + progressive achievement system

-- ─── 渐进成就配置表 ─────────────────────────────────────────────────────
CREATE TABLE achievement_progressive (
    id      VARCHAR(64)  PRIMARY KEY,
    name    VARCHAR(200) NOT NULL,
    mode    INT          NOT NULL,
    gap     INT          NOT NULL DEFAULT 5,
    init    INT          NOT NULL DEFAULT 1,
    rewards JSONB        NOT NULL DEFAULT '[]'
);

-- ─── 用户渐进成就进度 ────────────────────────────────────────────────────
CREATE TABLE user_progressive_achievement_progress (
    user_id        VARCHAR(12) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    achievement_id VARCHAR(64) NOT NULL REFERENCES achievement_progressive(id),
    step           INT         NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, achievement_id)
);

-- ─── 4个模式各一个渐进成就（每5关解锁，奖励递增）───────────────────────
INSERT INTO achievement_progressive (id, name, mode, gap, init, rewards) VALUES
  ('journey_level_milestone',   '征途通关成就', 1, 5, 1, '[{"itemId":"coin","count":1,"coefficient":5}]'),
  ('casual_level_milestone',    '休闲通关成就', 2, 5, 1, '[{"itemId":"coin","count":1,"coefficient":5}]'),
  ('ranked_level_milestone',    '竞技通关成就', 3, 5, 1, '[{"itemId":"coin","count":1,"coefficient":5}]'),
  ('challenge_level_milestone', '挑战通关成就', 4, 5, 1, '[{"itemId":"coin","count":1,"coefficient":5}]');

-- ─── 故事任务（链式：购买 -> 通关）────────────────────────────────────────
-- 先插入后者，避免 FK 冲突
INSERT INTO quests (id, quest_type, title, description, condition_type, condition_value, reward_item_id, reward_quantity, sort_order, next_quest_id) VALUES
  ('story_complete_level_1', 'story', '初探征途', '完成第一个关卡',   'level_completed', 1, 'coin', 20, 11, NULL);

INSERT INTO quests (id, quest_type, title, description, condition_type, condition_value, reward_item_id, reward_quantity, sort_order, next_quest_id) VALUES
  ('story_buy_shop_item',    'story', '初次购物', '购买一次商店物品', 'item_purchased',  1, 'coin', 10, 10, 'story_complete_level_1');

-- ─── 每日任务 ────────────────────────────────────────────────────────────
INSERT INTO quests (id, quest_type, title, description, condition_type, condition_value, reward_item_id, reward_quantity, sort_order, next_quest_id) VALUES
  ('daily_complete_level', 'daily', '每日关卡', '通过一个关卡',   'level_completed', 1, 'coin', 15, 10, NULL),
  ('daily_friend_invite',  'daily', '每日邀请', '创建一个邀请码', 'invite_created',  1, 'coin', 10, 11, NULL);
