# Spec: Notification Badge（红点系统）

## 目标

App 启动时一次性拉取所有模块的红点状态，避免多接口轮询；进入对应模块后清除。

## 数据模型

```sql
CREATE TABLE user_notification_badges (
    user_id VARCHAR(12) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    module  VARCHAR(32) NOT NULL,  -- quests | achievements | friends | inbox
    has_new BOOLEAN NOT NULL DEFAULT false,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, module)
);
```

## API 契约

### GET /api/notifications/badges
**Auth**: SessionToken required

**Response 200**
```json
{
  "quests":       true,
  "achievements": false,
  "friends":      false,
  "inbox":        true
}
```

### POST /api/notifications/badges/{module}/clear
**Auth**: SessionToken required  
**Path**: module ∈ {quests, achievements, friends, inbox}

**Response 200** `{}`  
**Response 400** module 不合法

## 红点设置时机

| 事件 | 设置哪个模块 |
|------|------------|
| 任何 story/daily quest completed | `quests` |
| 渐进成就里程碑达成 | `achievements` |
| 好友请求收到 | `friends` |
| 写入 user_inbox | `inbox` |

## 红点清除时机

| 操作 | 清除哪个模块 |
|------|------------|
| 客户端打开任务页面 | `quests` |
| 客户端打开成就页面 | `achievements` |
| 客户端打开好友页面 | `friends` |
| 客户端打开 Inbox | `inbox` |

## 实现要点

- 写入红点：在对应业务事务提交后调用（不在同一事务内，红点丢失可接受）
- 读取：直接查表，无缓存（频率低，一次拉取）
- `upsert ON CONFLICT DO UPDATE SET has_new = true, updated_at = NOW()`
