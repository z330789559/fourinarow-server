# Player Cache Management

- 玩家读写统一入口：新代码优先走 `DatabaseManager.players` / `PlayerRepository`，避免绕过 repository 直接改 `users.skill_rating`、`user_inventory`、`user_quest_progress`。
- 本机数据库容器名按用户说明是 `pg`，端口映射 `5432`；不要自动启动/停止容器。
- REST 会话请求头实际名称是 `SessionToken`；错误文案里仍可能出现历史 `session_token`。
- 商城购买支持可选 `Idempotency-Key`，repository 使用用户作用域幂等键 `purchase:<user_id>:<operation_id>`，header 长度上限 96。
- 邀请兑换使用结构化 `RedeemError`；业务错误返回 400，缓存/数据库错误记录日志并返回 500。
- `PLAYER_CACHE_FLUSH_IMMEDIATELY` 默认同步刷盘；设为 `0` 或 `false` 后启用延迟 write-behind。关闭服务会调用 `PlayerRepository::flush_all`。
- 当前最终 write-behind 范围：只有 `Stats` 真正接入生产异步写。`Profile` 保留机制但当前无生产写入口；`GameInfo/SR` 保持同步事务，避免天梯崩溃丢分；`Inventory / Quests / Achievements` 保持同步事务 + ledger + 幂等。
- 对局结算使用 `PlayedGameInfo.settlement_id` 作为 `game_settlement:<settlement_id>` 幂等键。事务内提交 SR、games、任务/成就、奖励和资产流水；提交后 reload 双方 aggregate 覆盖 cache，再通过 `with_player_mut([Stats])` 累加统计并标脏。不要恢复结算后无条件 `cache.remove`。
- 任务 story/daily 奖励已改为手动领取：任务事件只推进 `current_value`/`completed_at`，完成后保持 `rewarded=false`；前端调用 `POST /api/quests/story/{quest_id}/claim` 或 `POST /api/quests/daily/{quest_id}/claim` 后，`PlayerRepository::claim_quest_reward` 在事务内发奖、写 `asset_ledger`、写 `player_operation_idempotency`、置 `rewarded=true` 并 reload cache。成就奖励当前仍自动发放。
- `add_item_once` / `purchase` / `redeem_invite` 操作前保留 `flush_player`，提交后调用 reload 覆盖 cache；reload 失败时记录日志并移除 cache，避免旧快照继续被读。
- `UserManager` 在结算尝试后统一失效双方 `UserCollection` 缓存；即使 post-commit stats 阶段失败，也不会继续读旧 SR TTL 快照。
- dirty flush 使用绝对值写回：Inventory 按 aggregate 当前正数余额替换用户背包，Stats 按 aggregate 当前统计 upsert 并递增版本；不要恢复基于 `persisted_inventory` / `persisted_stats` 的 delta flush。
- 单玩家多个 dirty bucket flush 时，即使某个 bucket 失败也应继续尝试其他 bucket，最后统一返回错误；失败 bucket 保留 dirty 并递增 attempts。
- Stats 如果在延迟模式下崩溃丢计数，可从 `games` 表重算并回填 `player_stats`；验证 SQL 写在 `docs/player_cache_management_verification.md`。
- focused 验证命令：`cargo test player::aggregate -- --nocapture`、`cargo test player::repository -- --nocapture`、`cargo test player::repository::tests::db_ -- --ignored --nocapture`、`cargo check`、`openspec validate "player-cache-management" --strict`、`openspec validate "manual-quest-reward-claim" --strict`。