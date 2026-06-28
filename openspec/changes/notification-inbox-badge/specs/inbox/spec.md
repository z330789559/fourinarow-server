# Spec: 站内信 Inbox

## 目标

统一承载所有需要"持久化+用户感知"的通知，部分通知含可领取奖励。解决 HTTP 模式下通知完全缺失的问题。

## 数据模型

```sql
CREATE TABLE user_inbox (
    id             BIGSERIAL    PRIMARY KEY,
    user_id        VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    type           VARCHAR(40)  NOT NULL,
    title          VARCHAR(200) NOT NULL,
    body           TEXT         NOT NULL DEFAULT '',
    reward_item_id VARCHAR(50)  REFERENCES items(id),
    reward_qty     INT          NOT NULL DEFAULT 0,
    claimed        BOOLEAN      NOT NULL DEFAULT false,
    read           BOOLEAN      NOT NULL DEFAULT false,
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    expires_at     TIMESTAMPTZ  -- NULL = 永不过期（本期不做清理）
);

CREATE INDEX idx_user_inbox_user ON user_inbox(user_id, created_at DESC);
```

## 消息类型（type 枚举）

| type | 有奖励 | 触发场景 |
|------|--------|---------|
| `quest_reward` | 可选 | （本期不改，任务奖励保持 claim 接口）|
| `progressive_achievement` | ✓ | 渐进成就里程碑（替代直接发放背包）|
| `achievement_reward` | 可选 | 固定成就解锁奖励 |
| `battle_settlement` | ✗ | 四子棋结算结果（纯告知）|
| `system_gift` | ✓ | 运营系统发放 |

## API 契约

### GET /api/inbox
**Auth**: SessionToken required  
**Query**: `?unread_only=true`（默认 false，全量）

**Response 200**
```json
[
  {
    "id": 42,
    "type": "progressive_achievement",
    "title": "征途通关成就 · 第1里程碑",
    "body": "恭喜通过征途第5关！",
    "reward_item_id": "coin",
    "reward_qty": 5,
    "claimed": false,
    "read": false,
    "created_at": "2026-06-24T07:00:00Z"
  }
]
```

### POST /api/inbox/{id}/claim
**Auth**: SessionToken required  
领取奖励：将 `reward_item_id / reward_qty` 原子转账到 `user_inventory` + `asset_ledger`，设置 `claimed = true`

**Response 200** `{"reward_item_id":"coin","reward_qty":5}`  
**Response 404** 消息不存在 or 不属于当前用户  
**Response 409** 已领取  
**Response 400** 无奖励可领

### POST /api/inbox/{id}/read
**Auth**: SessionToken required  
标记已读（`read = true`），不影响 claimed

**Response 200** `{}`

### DELETE /api/inbox/{id}
**Auth**: SessionToken required  
删除消息（若有奖励未领取则拒绝删除）

**Response 200** `{}`  
**Response 400** 奖励未领取，请先领取

## 幂等性

- `claim` 使用 `asset_ledger.idempotency_key = "inbox_claim:{id}"` 保证幂等
- 多次点击 claim 返回 409，前端静默处理

## 渐进成就写入 Inbox 的消息格式

- title: `"{achievement_name} · 第{step}里程碑"`
- body: `"恭喜通过第{target_level}关！"`
- reward_item_id / reward_qty: 按 config 计算

## 实现要点

- `check_progressive_achievements_in_tx` 不再写 asset_ledger / add_inventory，改为 INSERT INTO user_inbox
- claim 接口内执行 asset_ledger INSERT + add_inventory + claimed=true（同一事务）
- 写入 inbox 后：触发 `new_inbox_message` WS 推送 + 设置 `inbox` 红点
