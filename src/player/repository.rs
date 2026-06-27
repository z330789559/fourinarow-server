use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, Utc};
use dashmap::DashMap;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use sqlx::{PgPool, Postgres, Transaction};
use tokio::time::{self, Duration};

use crate::api::users::user::{UserGameInfo, UserId};
use crate::config::GameConfig;
use crate::database::items::InventoryEntry;
use crate::player::aggregate::{
    DirtyBucket, PlayerAchievementProgress, PlayerAggregate, PlayerCacheEntry, PlayerProfile,
    PlayerQuestProgress, PlayerStats,
};
use crate::quests::GameEvent;

const FLUSH_TICK_MS: u64 = 500;
const FLUSH_COOLDOWN_SECS: i64 = 5;
const FORCE_FLUSH_SECS: i64 = 10 * 60;

#[derive(Clone)]
pub struct PlayerRepository {
    pool: PgPool,
    cache: Arc<DashMap<UserId, PlayerCacheEntry>>,
    flush_immediately: bool,
}

#[derive(Debug)]
pub enum PlayerRepositoryError {
    NotFound,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for PlayerRepositoryError {
    fn from(error: sqlx::Error) -> Self {
        PlayerRepositoryError::Db(error)
    }
}

#[derive(Debug)]
pub enum FlushError {
    Players(Vec<(UserId, PlayerRepositoryError)>),
}

#[derive(Debug)]
pub enum PurchaseError {
    ItemNotFound,
    NotEnoughItems,
    AlreadyApplied,
    CacheFlush(PlayerRepositoryError),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for PurchaseError {
    fn from(error: sqlx::Error) -> Self {
        PurchaseError::Db(error)
    }
}

#[derive(Debug)]
pub enum RedeemError {
    InvalidCode,
    Expired,
    MaxUsesReached,
    AlreadyUsed,
    CacheFlush(PlayerRepositoryError),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for RedeemError {
    fn from(error: sqlx::Error) -> Self {
        RedeemError::Db(error)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum QuestClaimKind {
    Story,
    Daily,
}

#[derive(Debug, Clone)]
pub struct QuestClaimReward {
    pub quest_id: String,
    pub reward_item_id: Option<String>,
    pub reward_quantity: i32,
}

#[derive(Debug)]
pub enum QuestClaimError {
    NotFound,
    NotCompleted,
    AlreadyClaimed,
    CacheFlush(PlayerRepositoryError),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for QuestClaimError {
    fn from(error: sqlx::Error) -> Self {
        QuestClaimError::Db(error)
    }
}

#[derive(Debug)]
pub enum SettlementError {
    MissingWinner,
    MissingLoser,
    CacheFlush(PlayerRepositoryError),
    CacheReload(PlayerRepositoryError),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for SettlementError {
    fn from(error: sqlx::Error) -> Self {
        SettlementError::Db(error)
    }
}

#[derive(Debug)]
pub enum CompleteLevelError {
    ModeNotUnlocked,
    NotNextLevel,
    InvalidStars,
    ModeMissing,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for CompleteLevelError {
    fn from(error: sqlx::Error) -> Self {
        CompleteLevelError::Db(error)
    }
}

#[derive(Debug, Clone)]
pub struct CompleteLevelOutcome {
    pub new_level: i32,
    pub rewards: Vec<(String, i32)>,
    pub achievements: Vec<(String, i32)>,
    pub milestones: Vec<(String, i32, i64)>,    // (achievement_id, step_reached, inbox_id)
    pub completed_quests: Vec<(String, String)>, // (quest_id, quest_type)
}

#[derive(Debug, Clone)]
pub struct SettlementOutcome {
    pub winner_rewards: Vec<(String, i32)>,
    pub loser_rewards: Vec<(String, i32)>,
    /// Achievement tiers newly completed by the winner: (achievement_id, tier_completed)
    pub winner_achievements: Vec<(String, i32)>,
    pub winner_completed_quests: Vec<(String, String)>,
    pub loser_completed_quests: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct DbQuest {
    id: String,
    condition_value: i32,
}

impl PlayerRepository {
    pub fn new(pool: PgPool) -> Self {
        let flush_immediately = std::env::var("PLAYER_CACHE_FLUSH_IMMEDIATELY")
            .map(|value| value != "0" && value.to_lowercase() != "false")
            .unwrap_or(true);

        PlayerRepository {
            pool,
            cache: Arc::new(DashMap::new()),
            flush_immediately,
        }
    }

    pub fn start_flush_worker(&self) {
        let repository = self.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(FLUSH_TICK_MS));
            loop {
                interval.tick().await;
                if let Err(error) = repository.tick_flush().await {
                    log::error!("player cache tick flush failed: {:?}", error);
                }
            }
        });
    }

    pub async fn get_readonly(
        &self,
        user_id: &UserId,
    ) -> Result<PlayerAggregate, PlayerRepositoryError> {
        if let Some(entry) = self.cache.get(user_id) {
            return Ok(entry.aggregate.clone());
        }

        let aggregate = self.load_aggregate(user_id).await?;
        self.cache
            .insert(*user_id, PlayerCacheEntry::clean(aggregate.clone()));
        Ok(aggregate)
    }

    pub async fn with_player_mut<F, R>(
        &self,
        user_id: &UserId,
        buckets: &[DirtyBucket],
        reason: &str,
        f: F,
    ) -> Result<R, PlayerRepositoryError>
    where
        F: FnOnce(&mut PlayerAggregate) -> R,
    {
        if !self.cache.contains_key(user_id) {
            let aggregate = self.load_aggregate(user_id).await?;
            self.cache
                .entry(*user_id)
                .or_insert_with(|| PlayerCacheEntry::clean(aggregate));
        }

        let (result, dirty_buckets) = {
            let mut entry = self
                .cache
                .get_mut(user_id)
                .ok_or(PlayerRepositoryError::NotFound)?;
            let result = f(&mut entry.aggregate);
            entry.mark_dirty(buckets.iter().copied(), Some(reason));
            let dirty_buckets = entry.dirty_buckets();
            (result, dirty_buckets)
        };

        if self.flush_immediately {
            self.flush_player_buckets(user_id, dirty_buckets).await?;
        }

        Ok(result)
    }

    async fn reload_player(
        &self,
        user_id: &UserId,
        reason: &str,
    ) -> Result<PlayerAggregate, PlayerRepositoryError> {
        let aggregate = self.load_aggregate(user_id).await?;
        self.cache
            .insert(*user_id, PlayerCacheEntry::clean(aggregate.clone()));
        log::debug!(
            "reloaded player cache user_id={} reason={}",
            user_id,
            reason
        );
        Ok(aggregate)
    }

    async fn reload_player_after_sync_write(&self, user_id: &UserId, reason: &str) {
        if let Err(error) = self.reload_player(user_id, reason).await {
            log::error!(
                "failed to reload player cache after sync write user_id={} reason={}: {:?}",
                user_id,
                reason,
                error
            );
            self.cache.remove(user_id);
        }
    }

    async fn bump_stats(
        &self,
        user_id: &UserId,
        played_delta: i32,
        won_delta: i32,
        lost_delta: i32,
        reason: &str,
    ) -> Result<(), PlayerRepositoryError> {
        self.reload_player(user_id, reason).await?;
        self.with_player_mut(user_id, &[DirtyBucket::Stats], reason, |player| {
            player.stats.games_played += played_delta;
            player.stats.games_won += won_delta;
            player.stats.games_lost += lost_delta;
        })
        .await
    }

    pub async fn flush_player(&self, user_id: &UserId) -> Result<(), PlayerRepositoryError> {
        let Some(entry) = self.cache.get(user_id) else {
            return Ok(());
        };
        let buckets = entry.dirty_buckets();
        drop(entry);
        self.flush_player_buckets(user_id, buckets).await
    }

    pub async fn flush_all(&self) -> Result<(), FlushError> {
        let ids: Vec<UserId> = self.cache.iter().map(|entry| *entry.key()).collect();
        let mut failures = Vec::new();
        for id in ids {
            if let Err(error) = self.flush_player(&id).await {
                log::error!(
                    "failed to flush player {} during flush_all: {:?}",
                    id,
                    error
                );
                failures.push((id, error));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(FlushError::Players(failures))
        }
    }

    async fn tick_flush(&self) -> Result<(), PlayerRepositoryError> {
        let now = Utc::now();
        let due: Vec<(UserId, BTreeSet<DirtyBucket>)> = self
            .cache
            .iter()
            .filter_map(|entry| {
                let buckets: BTreeSet<DirtyBucket> = entry
                    .dirty
                    .iter()
                    .filter_map(|(bucket, state)| {
                        let since_change = now - state.changed_at;
                        let since_flush = now - state.last_flush_at;
                        if since_change >= ChronoDuration::seconds(FLUSH_COOLDOWN_SECS)
                            || since_flush >= ChronoDuration::seconds(FORCE_FLUSH_SECS)
                        {
                            Some(*bucket)
                        } else {
                            None
                        }
                    })
                    .collect();
                if buckets.is_empty() {
                    None
                } else {
                    Some((*entry.key(), buckets))
                }
            })
            .collect();

        for (id, buckets) in due {
            if let Err(error) = self.flush_player_buckets(&id, buckets).await {
                log::error!("failed to flush dirty player {}: {:?}", id, error);
            }
        }
        Ok(())
    }

    async fn flush_player_buckets(
        &self,
        user_id: &UserId,
        buckets: BTreeSet<DirtyBucket>,
    ) -> Result<(), PlayerRepositoryError> {
        if buckets.is_empty() {
            return Ok(());
        }

        let Some(entry) = self.cache.get(user_id) else {
            return Ok(());
        };
        let aggregate = entry.aggregate.clone();
        let snapshot_dirty = buckets
            .iter()
            .filter_map(|bucket| {
                entry
                    .dirty
                    .get(bucket)
                    .map(|state| (*bucket, state.changed_at))
            })
            .collect::<BTreeMap<_, _>>();
        drop(entry);

        let mut flush_errors = Vec::new();
        for bucket in buckets {
            let result = self.save_bucket(&aggregate, bucket).await;
            match result {
                Ok(()) => {
                    if let Some(mut cached) = self.cache.get_mut(user_id) {
                        let unchanged = cached
                            .dirty
                            .get(&bucket)
                            .and_then(|state| {
                                snapshot_dirty
                                    .get(&bucket)
                                    .map(|at| state.changed_at == *at)
                            })
                            .unwrap_or(false);
                        if unchanged {
                            cached.mark_bucket_flushed(bucket);
                        }
                    }
                }
                Err(error) => {
                    if let Some(mut cached) = self.cache.get_mut(user_id) {
                        if let Some(state) = cached.dirty.get_mut(&bucket) {
                            state.attempts += 1;
                        }
                    }
                    log::error!(
                        "failed to flush player {} bucket {:?}: {:?}",
                        user_id,
                        bucket,
                        error
                    );
                    flush_errors.push(error);
                }
            }
        }

        if let Some(error) = flush_errors.into_iter().next() {
            Err(error)
        } else {
            Ok(())
        }
    }

    async fn save_bucket(
        &self,
        aggregate: &PlayerAggregate,
        bucket: DirtyBucket,
    ) -> Result<(), PlayerRepositoryError> {
        match bucket {
            DirtyBucket::Profile => {
                sqlx::query(
                    "UPDATE users SET username = $1, email = $2, updated_at = NOW() WHERE id = $3",
                )
                .bind(&aggregate.profile.username)
                .bind(&aggregate.profile.email)
                .bind(aggregate.profile.id.to_string())
                .execute(&self.pool)
                .await?;
            }
            DirtyBucket::GameInfo => {
                sqlx::query("UPDATE users SET skill_rating = $1, updated_at = NOW() WHERE id = $2")
                    .bind(aggregate.game_info.skill_rating)
                    .bind(aggregate.profile.id.to_string())
                    .execute(&self.pool)
                    .await?;
            }
            DirtyBucket::Inventory => {
                let mut tx = self.pool.begin().await?;
                sqlx::query("DELETE FROM user_inventory WHERE user_id = $1")
                    .bind(aggregate.profile.id.to_string())
                    .execute(&mut *tx)
                    .await?;
                for (item_id, qty) in &aggregate.inventory {
                    if *qty > 0 {
                        set_inventory_quantity_in_tx(&mut tx, &aggregate.profile.id, item_id, *qty)
                            .await?;
                    }
                }
                tx.commit().await?;
            }
            DirtyBucket::Stats => {
                sqlx::query(
                    "INSERT INTO player_stats \
                     (user_id, games_played, games_won, games_lost, version, updated_at) \
                     VALUES ($1, $2, $3, $4, 1, NOW()) \
                     ON CONFLICT (user_id) DO UPDATE SET \
                     games_played = EXCLUDED.games_played, \
                     games_won = EXCLUDED.games_won, \
                     games_lost = EXCLUDED.games_lost, \
                     version = player_stats.version + 1, updated_at = NOW()",
                )
                .bind(aggregate.profile.id.to_string())
                .bind(aggregate.stats.games_played)
                .bind(aggregate.stats.games_won)
                .bind(aggregate.stats.games_lost)
                .execute(&self.pool)
                .await?;
            }
            DirtyBucket::Quests => {
                let mut tx = self.pool.begin().await?;
                for quest in &aggregate.quests {
                    sqlx::query(
                        "INSERT INTO user_quest_progress \
                         (user_id, quest_id, current_value, completed_at, rewarded, quest_date) \
                         VALUES ($1, $2, $3, $4, $5, $6) \
                         ON CONFLICT (user_id, quest_id, COALESCE(quest_date, '1970-01-01'::date)) \
                         DO UPDATE SET current_value = EXCLUDED.current_value, \
                         completed_at = EXCLUDED.completed_at, rewarded = EXCLUDED.rewarded",
                    )
                    .bind(aggregate.profile.id.to_string())
                    .bind(&quest.quest_id)
                    .bind(quest.current_value)
                    .bind(quest.completed_at)
                    .bind(quest.rewarded)
                    .bind(quest.quest_date)
                    .execute(&mut *tx)
                    .await?;
                }
                tx.commit().await?;
            }
            DirtyBucket::Achievements => {
                let mut tx = self.pool.begin().await?;
                for achievement in &aggregate.achievements {
                    sqlx::query(
                        "INSERT INTO user_achievement_progress \
                         (user_id, achievement_id, current_tier, current_value) \
                         VALUES ($1, $2, $3, $4) \
                         ON CONFLICT (user_id, achievement_id) DO UPDATE SET \
                         current_tier = EXCLUDED.current_tier, current_value = EXCLUDED.current_value",
                    )
                    .bind(aggregate.profile.id.to_string())
                    .bind(&achievement.achievement_id)
                    .bind(achievement.current_tier)
                    .bind(achievement.current_value)
                    .execute(&mut *tx)
                    .await?;
                }
                tx.commit().await?;
            }
        }
        Ok(())
    }

    async fn load_aggregate(
        &self,
        user_id: &UserId,
    ) -> Result<PlayerAggregate, PlayerRepositoryError> {
        let row: Option<(String, String, Option<String>, i32)> = sqlx::query_as(
            "SELECT id, username, email, skill_rating FROM users WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        let (id, username, email, skill_rating) = row.ok_or(PlayerRepositoryError::NotFound)?;
        let parsed_id = UserId::from_str(&id).map_err(|_| PlayerRepositoryError::NotFound)?;

        let inventory_rows: Vec<InventoryEntry> = sqlx::query_as(
            "SELECT item_id, quantity FROM user_inventory WHERE user_id = $1 AND quantity > 0",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let inventory = inventory_rows
            .into_iter()
            .map(|entry| (entry.item_id, entry.quantity))
            .collect::<BTreeMap<_, _>>();

        let quest_rows: Vec<(String, i32, Option<DateTime<Utc>>, bool, Option<NaiveDate>)> =
            sqlx::query_as(
                "SELECT quest_id, current_value, completed_at, rewarded, quest_date \
                 FROM user_quest_progress WHERE user_id = $1",
            )
            .bind(user_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        let quests = quest_rows
            .into_iter()
            .map(
                |(quest_id, current_value, completed_at, rewarded, quest_date)| {
                    PlayerQuestProgress {
                        quest_id,
                        current_value,
                        completed_at,
                        rewarded,
                        quest_date,
                    }
                },
            )
            .collect();

        let achievement_rows: Vec<(String, i32, i32)> = sqlx::query_as(
            "SELECT achievement_id, current_tier, current_value FROM user_achievement_progress \
             WHERE user_id = $1",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let achievements = achievement_rows
            .into_iter()
            .map(
                |(achievement_id, current_tier, current_value)| PlayerAchievementProgress {
                    achievement_id,
                    current_tier,
                    current_value,
                },
            )
            .collect();

        let stats = load_or_create_stats(&self.pool, user_id).await?;

        let progress_rows: Vec<(i32, i32)> = sqlx::query_as(
            "SELECT mode, level FROM player_mode_progress WHERE user_id = $1",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mode_progress: BTreeMap<i32, i32> = progress_rows.into_iter().collect();

        Ok(PlayerAggregate {
            profile: PlayerProfile {
                id: parsed_id,
                username,
                email,
            },
            game_info: UserGameInfo { skill_rating },
            inventory,
            quests,
            achievements,
            stats,
            mode_progress,
        })
    }

    pub async fn add_item_once(
        &self,
        user_id: &UserId,
        item_id: &str,
        qty: i32,
        source: &str,
        business_id: &str,
        idempotency_key: &str,
    ) -> Result<bool, PlayerRepositoryError> {
        if qty <= 0 {
            return Ok(false);
        }

        self.flush_player(user_id).await?;

        let mut tx = self.pool.begin().await?;
        let inserted = insert_idempotency(
            &mut tx,
            user_id,
            "asset_reward",
            business_id,
            idempotency_key,
        )
        .await?;
        if !inserted {
            tx.rollback().await?;
            return Ok(false);
        }

        add_inventory_in_tx(&mut tx, user_id, item_id, qty).await?;
        insert_asset_ledger(
            &mut tx,
            user_id,
            item_id,
            qty,
            source,
            business_id,
            idempotency_key,
        )
        .await?;
        tx.commit().await?;

        self.reload_player_after_sync_write(user_id, "asset_reward")
            .await;
        Ok(true)
    }

    pub async fn purchase(
        &self,
        user_id: &UserId,
        shop_id: &str,
        item_id: &str,
        request_id: Option<&str>,
    ) -> Result<Vec<(String, String)>, PurchaseError> {
        self.flush_player(user_id)
            .await
            .map_err(PurchaseError::CacheFlush)?;

        let operation_id = request_id.map(str::to_string).unwrap_or_else(|| {
            let nonce: String = thread_rng()
                .sample_iter(&Alphanumeric)
                .take(24)
                .map(char::from)
                .collect();
            format!("{}:{}:{}:{nonce}", user_id, shop_id, item_id)
        });
        let idempotency_key = format!("purchase:{}:{operation_id}", user_id);
        let business_id = format!("{shop_id}:{item_id}");

        let mut tx = self.pool.begin().await?;
        let inserted =
            insert_idempotency(&mut tx, user_id, "purchase", &business_id, &idempotency_key)
                .await?;
        if !inserted {
            tx.rollback().await?;
            return Err(PurchaseError::AlreadyApplied);
        }

        let row: Option<(String, i32, Option<i32>)> = sqlx::query_as(
            "SELECT price_item_id, price, stock FROM shop_items \
             WHERE shop_id = $1 AND item_id = $2 AND enabled = true \
             FOR UPDATE",
        )
        .bind(shop_id)
        .bind(item_id)
        .fetch_optional(&mut *tx)
        .await?;

        let (price_item_id, price, stock) = row.ok_or(PurchaseError::ItemNotFound)?;
        if stock.is_some_and(|stock_left| stock_left <= 0) {
            tx.rollback().await?;
            return Err(PurchaseError::NotEnoughItems);
        }

        let consumed = consume_inventory_in_tx(&mut tx, user_id, &price_item_id, price).await?;
        if !consumed {
            tx.rollback().await?;
            return Err(PurchaseError::NotEnoughItems);
        }

        add_inventory_in_tx(&mut tx, user_id, item_id, 1).await?;
        insert_asset_ledger(
            &mut tx,
            user_id,
            &price_item_id,
            -price,
            "purchase",
            &business_id,
            &idempotency_key,
        )
        .await?;
        insert_asset_ledger(
            &mut tx,
            user_id,
            item_id,
            1,
            "purchase",
            &business_id,
            &idempotency_key,
        )
        .await?;

        if stock.is_some() {
            sqlx::query(
                "UPDATE shop_items SET stock = stock - 1 WHERE shop_id = $1 AND item_id = $2 AND stock > 0",
            )
            .bind(shop_id)
            .bind(item_id)
            .execute(&mut *tx)
            .await?;
        }

        let purchase_quest_prefix = format!("purchase_quest:{shop_id}:{item_id}");
        let (_, _, completed_quests) = apply_quest_event_in_tx(&mut tx, user_id, &GameEvent::ItemPurchased, &purchase_quest_prefix).await?;

        tx.commit().await?;
        self.reload_player_after_sync_write(user_id, "purchase")
            .await;
        Ok(completed_quests)
    }

    pub async fn trigger_quest_event(
        &self,
        user_id: &UserId,
        event: GameEvent,
        prefix: &str,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let (_, _, completed_quests) = apply_quest_event_in_tx(&mut tx, user_id, &event, prefix).await?;
        tx.commit().await?;
        self.cache.remove(user_id);
        Ok(completed_quests)
    }

    pub async fn redeem_invite(
        &self,
        code: &str,
        user_id: &UserId,
    ) -> Result<Option<(String, i32)>, RedeemError> {
        self.flush_player(user_id)
            .await
            .map_err(RedeemError::CacheFlush)?;

        let mut tx = self.pool.begin().await?;

        let invite: Option<(i32, i32, Option<String>, i32, Option<DateTime<Utc>>)> =
            sqlx::query_as(
                "SELECT max_uses, uses, reward_item_id, reward_quantity, expires_at \
             FROM invite_codes WHERE code = $1 FOR UPDATE",
            )
            .bind(code)
            .fetch_optional(&mut *tx)
            .await?;

        let Some((max_uses, uses, reward_item_id, reward_quantity, expires_at)) = invite else {
            tx.rollback().await.ok();
            return Err(RedeemError::InvalidCode);
        };

        if expires_at.is_some_and(|expires| Utc::now() > expires) {
            tx.rollback().await.ok();
            return Err(RedeemError::Expired);
        }

        if uses >= max_uses {
            tx.rollback().await.ok();
            return Err(RedeemError::MaxUsesReached);
        }

        let business_id = format!("{code}:{}", user_id);
        let idempotency_key = format!("invite:{business_id}");
        let inserted = insert_idempotency(
            &mut tx,
            user_id,
            "invite_redeem",
            &business_id,
            &idempotency_key,
        )
        .await
        .map_err(RedeemError::Db)?;
        if !inserted {
            tx.rollback().await.ok();
            return Err(RedeemError::AlreadyUsed);
        }

        let use_insert =
            sqlx::query("INSERT INTO invite_code_uses (code, used_by) VALUES ($1, $2)")
                .bind(code)
                .bind(user_id.to_string())
                .execute(&mut *tx)
                .await;
        if let Err(error) = use_insert {
            tx.rollback().await.ok();
            return Err(RedeemError::Db(error));
        }

        sqlx::query("UPDATE invite_codes SET uses = uses + 1 WHERE code = $1")
            .bind(code)
            .execute(&mut *tx)
            .await?;

        let reward = reward_item_id
            .filter(|_| reward_quantity > 0)
            .map(|item_id| (item_id, reward_quantity));
        if let Some((item_id, qty)) = &reward {
            add_inventory_in_tx(&mut tx, user_id, item_id, *qty).await?;
            insert_asset_ledger(
                &mut tx,
                user_id,
                item_id,
                *qty,
                "invite",
                &business_id,
                &idempotency_key,
            )
            .await?;
        }

        tx.commit().await?;
        self.reload_player_after_sync_write(user_id, "invite_redeem")
            .await;
        Ok(reward)
    }

    pub async fn claim_quest_reward(
        &self,
        user_id: &UserId,
        kind: QuestClaimKind,
        quest_id: &str,
    ) -> Result<QuestClaimReward, QuestClaimError> {
        self.flush_player(user_id)
            .await
            .map_err(QuestClaimError::CacheFlush)?;

        let today = Utc::now().date_naive();
        let mut tx = self.pool.begin().await?;

        let progress: Option<(Option<DateTime<Utc>>, bool, Option<String>, i32)> =
            match kind {
                QuestClaimKind::Story => sqlx::query_as(
                    "SELECT uqp.completed_at, uqp.rewarded, q.reward_item_id, q.reward_quantity \
                 FROM user_quest_progress uqp \
                 JOIN quests q ON q.id = uqp.quest_id \
                 WHERE uqp.user_id = $1 AND uqp.quest_id = $2 \
                 AND uqp.quest_date IS NULL AND q.quest_type = 'story' \
                 FOR UPDATE OF uqp",
                )
                .bind(user_id.to_string())
                .bind(quest_id)
                .fetch_optional(&mut *tx)
                .await?,
                QuestClaimKind::Daily => sqlx::query_as(
                    "SELECT uqp.completed_at, uqp.rewarded, q.reward_item_id, q.reward_quantity \
                 FROM user_quest_progress uqp \
                 JOIN quests q ON q.id = uqp.quest_id \
                 WHERE uqp.user_id = $1 AND uqp.quest_id = $2 \
                 AND uqp.quest_date = $3 AND q.quest_type = 'daily' \
                 FOR UPDATE OF uqp",
                )
                .bind(user_id.to_string())
                .bind(quest_id)
                .bind(today)
                .fetch_optional(&mut *tx)
                .await?,
            };

        let Some((completed_at, rewarded, reward_item_id, reward_quantity)) = progress else {
            tx.rollback().await.ok();
            return Err(QuestClaimError::NotFound);
        };
        if completed_at.is_none() {
            tx.rollback().await.ok();
            return Err(QuestClaimError::NotCompleted);
        }
        if rewarded {
            tx.rollback().await.ok();
            return Err(QuestClaimError::AlreadyClaimed);
        }

        let business_id = match kind {
            QuestClaimKind::Story => format!("quest_claim:story:{quest_id}"),
            QuestClaimKind::Daily => format!("quest_claim:daily:{quest_id}:{today}"),
        };
        let idempotency_key = match kind {
            QuestClaimKind::Story => format!("quest:{}:story:{quest_id}", user_id),
            QuestClaimKind::Daily => format!("quest:{}:daily:{quest_id}:{today}", user_id),
        };
        let inserted = insert_idempotency(
            &mut tx,
            user_id,
            "quest_reward",
            &business_id,
            &idempotency_key,
        )
        .await?;
        if !inserted {
            tx.rollback().await.ok();
            return Err(QuestClaimError::AlreadyClaimed);
        }

        let reward = reward_tuple(reward_item_id.as_deref(), reward_quantity);
        if let Some((item_id, qty)) = &reward {
            add_inventory_in_tx(&mut tx, user_id, item_id, *qty).await?;
            insert_asset_ledger(
                &mut tx,
                user_id,
                item_id,
                *qty,
                "quest",
                &business_id,
                &idempotency_key,
            )
            .await?;
        }

        match kind {
            QuestClaimKind::Story => {
                sqlx::query(
                    "UPDATE user_quest_progress SET rewarded = true \
                     WHERE user_id = $1 AND quest_id = $2 AND quest_date IS NULL",
                )
                .bind(user_id.to_string())
                .bind(quest_id)
                .execute(&mut *tx)
                .await?;
            }
            QuestClaimKind::Daily => {
                sqlx::query(
                    "UPDATE user_quest_progress SET rewarded = true \
                     WHERE user_id = $1 AND quest_id = $2 AND quest_date = $3",
                )
                .bind(user_id.to_string())
                .bind(quest_id)
                .bind(today)
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        self.reload_player_after_sync_write(user_id, "quest_claim")
            .await;

        let (reward_item_id, reward_quantity) = reward
            .map(|(item_id, qty)| (Some(item_id), qty))
            .unwrap_or((None, 0));
        Ok(QuestClaimReward {
            quest_id: quest_id.to_string(),
            reward_item_id,
            reward_quantity,
        })
    }

    pub async fn settle_game(
        &self,
        settlement_id: &str,
        winner_id: &UserId,
        loser_id: &UserId,
        sr_delta: i32,
    ) -> Result<SettlementOutcome, SettlementError> {
        self.flush_player(winner_id)
            .await
            .map_err(SettlementError::CacheFlush)?;
        self.flush_player(loser_id)
            .await
            .map_err(SettlementError::CacheFlush)?;

        let mut tx = self.pool.begin().await?;
        let idempotency_key = format!("game_settlement:{settlement_id}");
        let inserted = insert_idempotency(
            &mut tx,
            winner_id,
            "game_settlement",
            settlement_id,
            &idempotency_key,
        )
        .await?;
        if !inserted {
            tx.rollback().await?;
            log::info!(
                "skip duplicate game settlement settlement_id={} winner={} loser={}",
                settlement_id,
                winner_id,
                loser_id
            );
            return Ok(SettlementOutcome {
                winner_rewards: Vec::new(),
                loser_rewards: Vec::new(),
                winner_achievements: Vec::new(),
                winner_completed_quests: Vec::new(),
                loser_completed_quests: Vec::new(),
            });
        }

        let winner_exists =
            sqlx::query("SELECT id FROM users WHERE id = $1 AND deleted_at IS NULL")
                .bind(winner_id.to_string())
                .fetch_optional(&mut *tx)
                .await?
                .is_some();
        if !winner_exists {
            tx.rollback().await?;
            return Err(SettlementError::MissingWinner);
        }

        let loser_exists = sqlx::query("SELECT id FROM users WHERE id = $1 AND deleted_at IS NULL")
            .bind(loser_id.to_string())
            .fetch_optional(&mut *tx)
            .await?
            .is_some();
        if !loser_exists {
            tx.rollback().await?;
            return Err(SettlementError::MissingLoser);
        }

        sqlx::query(
            "UPDATE users SET skill_rating = skill_rating + $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(sr_delta)
        .bind(winner_id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE users SET skill_rating = skill_rating - $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(sr_delta)
        .bind(loser_id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO games (winner_id, loser_id) VALUES ($1, $2)")
            .bind(winner_id.to_string())
            .bind(loser_id.to_string())
            .execute(&mut *tx)
            .await?;

        let mut winner_rewards = Vec::new();
        let mut winner_achievements: Vec<(String, i32)> = Vec::new();
        let mut winner_completed_quests: Vec<(String, String)> = Vec::new();
        let mut loser_completed_quests: Vec<(String, String)> = Vec::new();

        let (wr1, wa1, cq1) =
            apply_quest_event_in_tx(&mut tx, winner_id, &GameEvent::GameWon, "game_settlement")
                .await?;
        winner_rewards.extend(wr1);
        winner_achievements.extend(wa1);
        winner_completed_quests.extend(cq1);

        let (wr2, wa2, cq2) = apply_quest_event_in_tx(
            &mut tx,
            winner_id,
            &GameEvent::GamePlayed,
            "game_settlement",
        )
        .await?;
        winner_rewards.extend(wr2);
        winner_achievements.extend(wa2);
        winner_completed_quests.extend(cq2);

        let (loser_rewards, _, loser_cq) =
            apply_quest_event_in_tx(&mut tx, loser_id, &GameEvent::GamePlayed, "game_settlement")
                .await?;
        loser_completed_quests.extend(loser_cq);

        tx.commit().await?;
        self.bump_stats(winner_id, 1, 1, 0, "game_settlement")
            .await
            .map_err(SettlementError::CacheReload)?;
        self.bump_stats(loser_id, 1, 0, 1, "game_settlement")
            .await
            .map_err(SettlementError::CacheReload)?;

        Ok(SettlementOutcome {
            winner_rewards,
            loser_rewards,
            winner_achievements,
            winner_completed_quests,
            loser_completed_quests,
        })
    }

    /// Task 5.8: Lazy backfill — inserts level=0 rows for every mode that has no row yet.
    /// Called from StartGame and complete_level; also used by Task 5.3 on registration.
    pub async fn ensure_mode_progress(
        &self,
        user_id: &UserId,
        game_config: &GameConfig,
    ) -> Result<(), sqlx::Error> {
        for mode_cfg in &game_config.modes {
            sqlx::query(
                "INSERT INTO player_mode_progress (user_id, mode, level, updated_at) \
                 VALUES ($1, $2, 0, NOW()) ON CONFLICT (user_id, mode) DO NOTHING",
            )
            .bind(user_id.to_string())
            .bind(mode_cfg.mode)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Task 5.6: Complete a level in a mode. Validates, updates progress, and applies quest/reward
    /// events transactionally. Returns CompleteLevelOutcome with new level and any earned rewards.
    pub async fn complete_level(
        &self,
        user_id: &UserId,
        mode: i32,
        level_id: i32,
        stars: i32,
    ) -> Result<CompleteLevelOutcome, CompleteLevelError> {
        if !(1..=3).contains(&stars) {
            return Err(CompleteLevelError::InvalidStars);
        }

        let idempotency_key = format!("level_complete:{user_id}:{mode}:{level_id}");
        let mut tx = self.pool.begin().await?;

        let inserted = insert_idempotency(
            &mut tx,
            user_id,
            "level_complete",
            &idempotency_key,
            &idempotency_key,
        )
        .await?;
        if !inserted {
            tx.rollback().await?;
            return Ok(CompleteLevelOutcome { new_level: level_id, rewards: vec![], achievements: vec![], milestones: vec![], completed_quests: vec![] });
        }

        let maybe_current: Option<(i32,)> = sqlx::query_as(
            "SELECT level FROM player_mode_progress WHERE user_id = $1 AND mode = $2 FOR UPDATE",
        )
        .bind(user_id.to_string())
        .bind(mode)
        .fetch_optional(&mut *tx)
        .await?;

        let current_level = match maybe_current {
            Some((lvl,)) => lvl,
            None => {
                tx.rollback().await?;
                return Err(CompleteLevelError::ModeMissing);
            }
        };

        if level_id != current_level + 1 {
            tx.rollback().await?;
            return Err(CompleteLevelError::NotNextLevel);
        }

        sqlx::query(
            "UPDATE player_mode_progress SET level = $1, updated_at = NOW() \
             WHERE user_id = $2 AND mode = $3",
        )
        .bind(level_id)
        .bind(user_id.to_string())
        .bind(mode)
        .execute(&mut *tx)
        .await?;

        // Apply quest progress and collect any in-tx rewards from quest/achievement unlocks
        let business_prefix = format!("level_complete:{mode}:{level_id}");
        let (rewards, achievements, completed_quests) =
            apply_quest_event_in_tx(&mut tx, user_id, &GameEvent::LevelCompleted, &business_prefix)
                .await?;

        // Check infinite progressive achievements for this mode — rewards go to inbox
        let milestones = check_progressive_achievements_in_tx(
            &mut tx,
            user_id,
            mode,
            level_id,
            &business_prefix,
        )
        .await?;

        tx.commit().await?;

        // Evict from cache so next read reloads updated mode_progress
        self.cache.remove(user_id);

        Ok(CompleteLevelOutcome { new_level: level_id, rewards, achievements, milestones, completed_quests })
    }
}

async fn load_or_create_stats(pool: &PgPool, user_id: &UserId) -> Result<PlayerStats, sqlx::Error> {
    let row: Option<(i32, i32, i32, i64)> = sqlx::query_as(
        "SELECT games_played, games_won, games_lost, version FROM player_stats WHERE user_id = $1",
    )
    .bind(user_id.to_string())
    .fetch_optional(pool)
    .await?;

    if let Some((games_played, games_won, games_lost, version)) = row {
        return Ok(PlayerStats {
            games_played,
            games_won,
            games_lost,
            version,
        });
    }

    sqlx::query("INSERT INTO player_stats (user_id) VALUES ($1) ON CONFLICT (user_id) DO NOTHING")
        .bind(user_id.to_string())
        .execute(pool)
        .await?;

    Ok(PlayerStats::default())
}

// ─── 渐进成就 ─────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct ProgressiveRewardConfig {
    #[serde(rename = "itemId")]
    item_id: String,
    count: i32,
    coefficient: i32,
}

pub struct ProgressiveAchievementState {
    pub achievement_id: String,
    pub name: String,
    pub mode: i32,
    pub step: i32,
    pub current_target_level: i32,
    pub reward_preview: Vec<(String, i32)>,
}

async fn check_progressive_achievements_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    mode: i32,
    new_level: i32,
    _business_prefix: &str,
) -> Result<Vec<(String, i32, i64)>, sqlx::Error> {
    let row: Option<(String, String, i32, i32, String)> = sqlx::query_as(
        "SELECT id, name, init, gap, rewards::text FROM achievement_progressive WHERE mode = $1",
    )
    .bind(mode)
    .fetch_optional(&mut **tx)
    .await?;

    let Some((achievement_id, achievement_name, init, gap, rewards_json)) = row else {
        return Ok(vec![]);
    };

    let reward_cfgs: Vec<ProgressiveRewardConfig> =
        serde_json::from_str(&rewards_json).unwrap_or_default();

    let step: i32 = sqlx::query_scalar(
        "INSERT INTO user_progressive_achievement_progress (user_id, achievement_id, step) \
         VALUES ($1, $2, 0) \
         ON CONFLICT (user_id, achievement_id) DO UPDATE \
         SET step = user_progressive_achievement_progress.step \
         RETURNING step",
    )
    .bind(user_id.to_string())
    .bind(&achievement_id)
    .fetch_one(&mut **tx)
    .await?;

    let mut current_step = step;
    let mut milestones: Vec<(String, i32, i64)> = vec![];

    loop {
        let threshold = (init + current_step) * gap;
        if new_level < threshold {
            break;
        }
        // Each milestone reward goes to inbox (one inbox message per reward item)
        for cfg in &reward_cfgs {
            let amount = (current_step + 1) * cfg.coefficient * cfg.count;
            let title = format!("{}达成！通关第{}关里程碑", achievement_name, threshold);
            let (inbox_id,): (i64,) = sqlx::query_as(
                "INSERT INTO user_inbox (user_id, type, title, body, reward_item_id, reward_qty) \
                 VALUES ($1, 'progressive_achievement', $2, '', $3, $4) RETURNING id",
            )
            .bind(user_id.to_string())
            .bind(&title)
            .bind(&cfg.item_id)
            .bind(amount)
            .fetch_one(&mut **tx)
            .await?;
            milestones.push((achievement_id.clone(), current_step + 1, inbox_id));
        }
        current_step += 1;
    }

    if current_step > step {
        sqlx::query(
            "UPDATE user_progressive_achievement_progress SET step = $1 \
             WHERE user_id = $2 AND achievement_id = $3",
        )
        .bind(current_step)
        .bind(user_id.to_string())
        .bind(&achievement_id)
        .execute(&mut **tx)
        .await?;
    }

    Ok(milestones)
}

impl PlayerRepository {
    pub async fn story_quest_progress(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<crate::database::quests::QuestProgress>, sqlx::Error> {
        let rows: Vec<(String, i32, Option<DateTime<Utc>>, bool)> = sqlx::query_as(
            "SELECT q.id, COALESCE(uqp.current_value, 0), uqp.completed_at, \
             COALESCE(uqp.rewarded, false) \
             FROM quests q \
             LEFT JOIN user_quest_progress uqp \
               ON q.id = uqp.quest_id AND uqp.user_id = $1 AND uqp.quest_date IS NULL \
             WHERE q.quest_type = 'story' \
             ORDER BY q.sort_order ASC",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(quest_id, current_value, completed_at, rewarded)| {
                crate::database::quests::QuestProgress {
                    quest_id,
                    current_value,
                    completed_at,
                    rewarded,
                    quest_date: None,
                }
            })
            .collect())
    }

    pub async fn daily_quest_progress(
        &self,
        user_id: &UserId,
        today: NaiveDate,
    ) -> Result<Vec<crate::database::quests::QuestProgress>, sqlx::Error> {
        let rows: Vec<(String, i32, Option<DateTime<Utc>>, bool)> = sqlx::query_as(
            "SELECT q.id, COALESCE(uqp.current_value, 0), uqp.completed_at, \
             COALESCE(uqp.rewarded, false) \
             FROM quests q \
             LEFT JOIN user_quest_progress uqp \
               ON q.id = uqp.quest_id AND uqp.user_id = $1 AND uqp.quest_date = $2 \
             WHERE q.quest_type = 'daily' \
             ORDER BY q.sort_order ASC",
        )
        .bind(user_id.to_string())
        .bind(today)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(quest_id, current_value, completed_at, rewarded)| {
                crate::database::quests::QuestProgress {
                    quest_id,
                    current_value,
                    completed_at,
                    rewarded,
                    quest_date: Some(today),
                }
            })
            .collect())
    }

    pub async fn get_progressive_achievement_progress(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<ProgressiveAchievementState>, sqlx::Error> {
        let rows: Vec<(String, String, i32, i32, i32, String, i32)> = sqlx::query_as(
            "SELECT ap.id, ap.name, ap.mode, ap.gap, ap.init, ap.rewards::text, \
             COALESCE(up.step, 0) \
             FROM achievement_progressive ap \
             LEFT JOIN user_progressive_achievement_progress up \
               ON ap.id = up.achievement_id AND up.user_id = $1 \
             ORDER BY ap.mode ASC",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let states = rows
            .into_iter()
            .map(|(id, name, mode, gap, init, rewards_json, step)| {
                let reward_cfgs: Vec<ProgressiveRewardConfig> =
                    serde_json::from_str(&rewards_json).unwrap_or_default();
                let current_target_level = (init + step) * gap;
                let reward_preview = reward_cfgs
                    .iter()
                    .map(|cfg| (cfg.item_id.clone(), (step + 1) * cfg.coefficient * cfg.count))
                    .collect();
                ProgressiveAchievementState {
                    achievement_id: id,
                    name,
                    mode,
                    step,
                    current_target_level,
                    reward_preview,
                }
            })
            .collect();

        Ok(states)
    }
}

async fn insert_idempotency(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    operation_type: &str,
    business_id: &str,
    idempotency_key: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO player_operation_idempotency \
         (idempotency_key, user_id, operation_type, business_id) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (idempotency_key) DO NOTHING",
    )
    .bind(idempotency_key)
    .bind(user_id.to_string())
    .bind(operation_type)
    .bind(business_id)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected() > 0)
}

async fn insert_asset_ledger(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    item_id: &str,
    delta: i32,
    source: &str,
    business_id: &str,
    idempotency_key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO asset_ledger (user_id, item_id, delta, source, idempotency_key, business_id) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (idempotency_key, item_id, delta) DO NOTHING",
    )
    .bind(user_id.to_string())
    .bind(item_id)
    .bind(delta)
    .bind(source)
    .bind(idempotency_key)
    .bind(business_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn add_inventory_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    item_id: &str,
    qty: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO user_inventory (user_id, item_id, quantity) VALUES ($1, $2, $3) \
         ON CONFLICT (user_id, item_id) DO UPDATE SET quantity = user_inventory.quantity + $3",
    )
    .bind(user_id.to_string())
    .bind(item_id)
    .bind(qty)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn set_inventory_quantity_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    item_id: &str,
    qty: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO user_inventory (user_id, item_id, quantity) VALUES ($1, $2, $3) \
         ON CONFLICT (user_id, item_id) DO UPDATE SET quantity = EXCLUDED.quantity",
    )
    .bind(user_id.to_string())
    .bind(item_id)
    .bind(qty)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn consume_inventory_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    item_id: &str,
    qty: i32,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE user_inventory SET quantity = quantity - $3 \
         WHERE user_id = $1 AND item_id = $2 AND quantity >= $3",
    )
    .bind(user_id.to_string())
    .bind(item_id)
    .bind(qty)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Returns `(item_rewards, unlocked_achievements, completed_quests)`.
/// `completed_quests` is `Vec<(quest_id, quest_type)>`.
async fn apply_quest_event_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    event: &GameEvent,
    business_prefix: &str,
) -> Result<(Vec<(String, i32)>, Vec<(String, i32)>, Vec<(String, String)>), sqlx::Error> {
    let mut rewards = Vec::new();
    let mut unlocked_achievements: Vec<(String, i32)> = Vec::new();
    let mut completed_quests: Vec<(String, String)> = Vec::new();
    let condition = event.condition_type();
    let today = Utc::now().date_naive();

    let story_quests = load_quests_for_condition(tx, "story", condition).await?;
    for quest in story_quests {
        let done = apply_story_quest_in_tx(tx, user_id, &quest).await?;
        if done {
            completed_quests.push((quest.id.clone(), "story".to_string()));
        }
    }

    let daily_quests = load_quests_for_condition(tx, "daily", condition).await?;
    for quest in daily_quests {
        let done = apply_daily_quest_in_tx(tx, user_id, &quest, today).await?;
        if done {
            completed_quests.push((quest.id.clone(), "daily".to_string()));
        }
    }

    if matches!(event, GameEvent::GameWon) {
        let (item_rewards, achieved) =
            apply_achievements_in_tx(tx, user_id, business_prefix).await?;
        rewards.extend(item_rewards);
        unlocked_achievements.extend(achieved);
    }

    Ok((rewards, unlocked_achievements, completed_quests))
}

async fn load_quests_for_condition(
    tx: &mut Transaction<'_, Postgres>,
    quest_type: &str,
    condition: &str,
) -> Result<Vec<DbQuest>, sqlx::Error> {
    sqlx::query_as::<_, (String, i32)>(
        "SELECT id, condition_value FROM quests \
         WHERE quest_type = $1 AND condition_type = $2 ORDER BY sort_order ASC",
    )
    .bind(quest_type)
    .bind(condition)
    .fetch_all(&mut **tx)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|(id, condition_value)| DbQuest {
                id,
                condition_value,
            })
            .collect()
    })
}

async fn apply_story_quest_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    quest: &DbQuest,
) -> Result<bool, sqlx::Error> {
    let maybe_prog: Option<(i32, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT current_value, completed_at FROM user_quest_progress \
         WHERE user_id = $1 AND quest_id = $2 AND quest_date IS NULL FOR UPDATE",
    )
    .bind(user_id.to_string())
    .bind(&quest.id)
    .fetch_optional(&mut **tx)
    .await?;

    match maybe_prog {
        None => {
            let accessible = story_quest_accessible_in_tx(tx, user_id, &quest.id).await?;
            if !accessible {
                return Ok(false);
            }
            let new_value = 1_i32;
            let completed = new_value >= quest.condition_value;
            sqlx::query(
                "INSERT INTO user_quest_progress (user_id, quest_id, current_value, completed_at, rewarded) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(user_id.to_string())
            .bind(&quest.id)
            .bind(new_value)
            .bind(if completed { Some(Utc::now()) } else { None })
            .bind(false)
            .execute(&mut **tx)
            .await?;
            Ok(completed)
        }
        Some((current_value, None)) => {
            let new_value = current_value + 1;
            let completed = new_value >= quest.condition_value;
            sqlx::query(
                "UPDATE user_quest_progress SET current_value = $1, completed_at = $2, rewarded = $3 \
                 WHERE user_id = $4 AND quest_id = $5 AND quest_date IS NULL",
            )
            .bind(new_value)
            .bind(if completed { Some(Utc::now()) } else { None })
            .bind(false)
            .bind(user_id.to_string())
            .bind(&quest.id)
            .execute(&mut **tx)
            .await?;
            Ok(completed)
        }
        Some((_, Some(_))) => Ok(false),
    }
}

async fn story_quest_accessible_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    quest_id: &str,
) -> Result<bool, sqlx::Error> {
    let is_first: (bool,) = sqlx::query_as(
        "SELECT NOT EXISTS(SELECT 1 FROM quests WHERE quest_type = 'story' AND next_quest_id = $1)",
    )
    .bind(quest_id)
    .fetch_one(&mut **tx)
    .await?;
    if is_first.0 {
        return Ok(true);
    }

    let prev_done: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM user_quest_progress uqp \
         JOIN quests q ON q.next_quest_id = $1 \
         WHERE uqp.user_id = $2 AND uqp.quest_id = q.id AND uqp.completed_at IS NOT NULL \
         AND uqp.quest_date IS NULL)",
    )
    .bind(quest_id)
    .bind(user_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    Ok(prev_done.0)
}

async fn apply_daily_quest_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    quest: &DbQuest,
    today: NaiveDate,
) -> Result<bool, sqlx::Error> {
    let maybe_prog: Option<(i32, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT current_value, completed_at FROM user_quest_progress \
         WHERE user_id = $1 AND quest_id = $2 AND quest_date = $3 FOR UPDATE",
    )
    .bind(user_id.to_string())
    .bind(&quest.id)
    .bind(today)
    .fetch_optional(&mut **tx)
    .await?;

    match maybe_prog {
        None => {
            let completed = 1 >= quest.condition_value;
            sqlx::query(
                "INSERT INTO user_quest_progress (user_id, quest_id, current_value, completed_at, rewarded, quest_date) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (user_id, quest_id, COALESCE(quest_date, '1970-01-01'::date)) DO NOTHING",
            )
            .bind(user_id.to_string())
            .bind(&quest.id)
            .bind(1_i32)
            .bind(if completed { Some(Utc::now()) } else { None })
            .bind(false)
            .bind(today)
            .execute(&mut **tx)
            .await?;
            Ok(completed)
        }
        Some((current_value, None)) => {
            let new_value = current_value + 1;
            let completed = new_value >= quest.condition_value;
            sqlx::query(
                "UPDATE user_quest_progress SET current_value = $1, completed_at = $2, rewarded = $3 \
                 WHERE user_id = $4 AND quest_id = $5 AND quest_date = $6",
            )
            .bind(new_value)
            .bind(if completed { Some(Utc::now()) } else { None })
            .bind(false)
            .bind(user_id.to_string())
            .bind(&quest.id)
            .bind(today)
            .execute(&mut **tx)
            .await?;
            Ok(completed)
        }
        Some((_, Some(_))) => Ok(false),
    }
}

/// Returns `(item_rewards, unlocked_achievements)` where unlocked_achievements is
/// a list of `(achievement_id, completed_tier)` for tiers newly completed this call.
async fn apply_achievements_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    business_prefix: &str,
) -> Result<(Vec<(String, i32)>, Vec<(String, i32)>), sqlx::Error> {
    let mut rewards = Vec::new();
    let mut unlocked_achievements: Vec<(String, i32)> = Vec::new();
    let achievements: Vec<String> = sqlx::query_as::<_, (String,)>(
        "SELECT DISTINCT achievement_id FROM achievement_tiers WHERE tier = 1 AND condition_value > 0",
    )
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|(id,)| id)
    .collect();

    for achievement_id in achievements {
        let maybe_prog: Option<(i32, i32)> = sqlx::query_as(
            "SELECT current_tier, current_value FROM user_achievement_progress \
             WHERE user_id = $1 AND achievement_id = $2 FOR UPDATE",
        )
        .bind(user_id.to_string())
        .bind(&achievement_id)
        .fetch_optional(&mut **tx)
        .await?;

        let (current_tier, current_value) = maybe_prog.unwrap_or((1, 0));
        let new_value = current_value + 1;
        let tier: Option<(i32, Option<String>, i32)> = sqlx::query_as(
            "SELECT condition_value, reward_item_id, reward_quantity FROM achievement_tiers \
             WHERE achievement_id = $1 AND tier = $2",
        )
        .bind(&achievement_id)
        .bind(current_tier)
        .fetch_optional(&mut **tx)
        .await?;

        let Some((target, reward_item_id, reward_quantity)) = tier else {
            continue;
        };

        if maybe_prog.is_none() {
            sqlx::query(
                "INSERT INTO user_achievement_progress (user_id, achievement_id, current_tier, current_value) \
                 VALUES ($1, $2, $3, $4) ON CONFLICT (user_id, achievement_id) DO NOTHING",
            )
            .bind(user_id.to_string())
            .bind(&achievement_id)
            .bind(current_tier)
            .bind(new_value)
            .execute(&mut **tx)
            .await?;
        } else {
            sqlx::query(
                "UPDATE user_achievement_progress SET current_value = $1 \
                 WHERE user_id = $2 AND achievement_id = $3",
            )
            .bind(new_value)
            .bind(user_id.to_string())
            .bind(&achievement_id)
            .execute(&mut **tx)
            .await?;
        }

        if new_value >= target {
            sqlx::query(
                "UPDATE user_achievement_progress SET current_tier = $1, current_value = 0 \
                 WHERE user_id = $2 AND achievement_id = $3",
            )
            .bind(current_tier + 1)
            .bind(user_id.to_string())
            .bind(&achievement_id)
            .execute(&mut **tx)
            .await?;

            unlocked_achievements.push((achievement_id.clone(), current_tier));

            if let Some((item_id, qty)) = reward_tuple(reward_item_id.as_deref(), reward_quantity) {
                let business_id =
                    format!("{business_prefix}:achievement:{achievement_id}:{current_tier}");
                let key = format!(
                    "achievement:{}:{}:{}",
                    user_id, achievement_id, current_tier
                );
                if insert_idempotency(tx, user_id, "achievement_reward", &business_id, &key).await?
                {
                    add_inventory_in_tx(tx, user_id, &item_id, qty).await?;
                    insert_asset_ledger(
                        tx,
                        user_id,
                        &item_id,
                        qty,
                        "achievement",
                        &business_id,
                        &key,
                    )
                    .await?;
                    rewards.push((item_id, qty));
                }
            }
        }
    }

    Ok((rewards, unlocked_achievements))
}

fn reward_tuple(reward_item_id: Option<&str>, reward_quantity: i32) -> Option<(String, i32)> {
    reward_item_id
        .filter(|_| reward_quantity > 0)
        .map(|item_id| (item_id.to_string(), reward_quantity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_user(id: &str) -> UserId {
        UserId::from_str(id).expect("valid test user id")
    }

    fn test_aggregate(user_id: UserId) -> PlayerAggregate {
        PlayerAggregate {
            profile: PlayerProfile {
                id: user_id,
                username: "repository-test".to_string(),
                email: None,
            },
            game_info: UserGameInfo { skill_rating: 1000 },
            inventory: BTreeMap::new(),
            quests: Vec::new(),
            achievements: Vec::new(),
            stats: PlayerStats::default(),
            mode_progress: BTreeMap::new(),
        }
    }

    fn failing_repository() -> PlayerRepository {
        let pool = PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(50))
            .connect_lazy("postgres://postgres:postgres@127.0.0.1:1/fourinarow")
            .expect("lazy pool");
        PlayerRepository {
            pool,
            cache: Arc::new(DashMap::new()),
            flush_immediately: false,
        }
    }

    #[tokio::test]
    async fn failed_flush_retains_dirty_bucket() {
        let repository = failing_repository();
        let user_id = test_user("000000000011");
        let mut entry = PlayerCacheEntry::clean(test_aggregate(user_id));
        entry.aggregate.game_info.skill_rating += 1;
        entry.mark_dirty([DirtyBucket::GameInfo], Some("unit-test"));
        repository.cache.insert(user_id, entry);

        let result = repository.flush_player(&user_id).await;

        assert!(result.is_err());
        let cached = repository.cache.get(&user_id).expect("cached player");
        let state = cached
            .dirty
            .get(&DirtyBucket::GameInfo)
            .expect("dirty bucket retained");
        assert_eq!(state.attempts, 1);
        assert_eq!(state.reason.as_deref(), Some("unit-test"));
    }

    #[tokio::test]
    async fn failed_flush_attempts_all_dirty_buckets_for_player() {
        let repository = failing_repository();
        let user_id = test_user("000000000012");
        let mut entry = PlayerCacheEntry::clean(test_aggregate(user_id));
        entry.aggregate.game_info.skill_rating += 1;
        entry.aggregate.inventory.insert("coin".to_string(), 1);
        entry.mark_dirty(
            [DirtyBucket::GameInfo, DirtyBucket::Inventory],
            Some("multi-bucket-test"),
        );
        repository.cache.insert(user_id, entry);

        let result = repository.flush_player(&user_id).await;

        assert!(result.is_err());
        let cached = repository.cache.get(&user_id).expect("cached player");
        for bucket in [DirtyBucket::GameInfo, DirtyBucket::Inventory] {
            let state = cached.dirty.get(&bucket).expect("dirty bucket retained");
            assert_eq!(state.attempts, 1);
            assert_eq!(state.reason.as_deref(), Some("multi-bucket-test"));
        }
    }

    #[tokio::test]
    async fn flush_all_attempts_every_dirty_player() {
        let repository = failing_repository();
        for id in ["000000000021", "000000000022"] {
            let user_id = test_user(id);
            let mut entry = PlayerCacheEntry::clean(test_aggregate(user_id));
            entry.aggregate.game_info.skill_rating += 1;
            entry.mark_dirty([DirtyBucket::GameInfo], Some("flush-all-test"));
            repository.cache.insert(user_id, entry);
        }

        let result = repository.flush_all().await;

        let Err(FlushError::Players(failures)) = result else {
            panic!("flush_all should report all player failures");
        };
        assert_eq!(failures.len(), 2);
    }

    #[tokio::test]
    #[ignore]
    async fn db_flush_all_persists_dirty_player() {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("connect database");
        let repository = PlayerRepository {
            pool: pool.clone(),
            cache: Arc::new(DashMap::new()),
            flush_immediately: false,
        };
        let user_id = test_user("000000000091");
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, email, skill_rating) \
             VALUES ($1, $2, '', NULL, 1000) \
             ON CONFLICT (id) DO UPDATE SET skill_rating = 1000, updated_at = NOW()",
        )
        .bind(user_id.to_string())
        .bind("pcm_db_flush_test")
        .execute(&pool)
        .await
        .expect("upsert test user");

        repository
            .get_readonly(&user_id)
            .await
            .expect("load aggregate");
        repository
            .with_player_mut(
                &user_id,
                &[DirtyBucket::GameInfo],
                "db-flush-test",
                |player| {
                    player.game_info.skill_rating += 13;
                },
            )
            .await
            .expect("mark dirty");

        repository.flush_all().await.expect("flush all");

        let (skill_rating,): (i32,) =
            sqlx::query_as("SELECT skill_rating FROM users WHERE id = $1")
                .bind(user_id.to_string())
                .fetch_one(&pool)
                .await
                .expect("select skill rating");
        assert_eq!(skill_rating, 1013);
    }

    #[tokio::test]
    #[ignore]
    async fn db_concurrent_inventory_flush_uses_absolute_quantity() {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
            .expect("connect database");
        let repository = PlayerRepository {
            pool: pool.clone(),
            cache: Arc::new(DashMap::new()),
            flush_immediately: false,
        };
        let user_id = test_user("000000000092");
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, email, skill_rating) \
             VALUES ($1, $2, '', NULL, 1000) \
             ON CONFLICT (id) DO UPDATE SET skill_rating = 1000, updated_at = NOW()",
        )
        .bind(user_id.to_string())
        .bind("pcm_inventory_flush_test")
        .execute(&pool)
        .await
        .expect("upsert test user");
        sqlx::query("DELETE FROM user_inventory WHERE user_id = $1")
            .bind(user_id.to_string())
            .execute(&pool)
            .await
            .expect("clean inventory");
        sqlx::query(
            "INSERT INTO user_inventory (user_id, item_id, quantity) VALUES ($1, 'coin', 90)",
        )
        .bind(user_id.to_string())
        .execute(&pool)
        .await
        .expect("seed inventory");

        let mut aggregate = test_aggregate(user_id);
        aggregate.inventory.insert("coin".to_string(), 150);
        let mut entry = PlayerCacheEntry::clean(aggregate);
        entry.mark_dirty([DirtyBucket::Inventory], Some("absolute-flush-test"));
        repository.cache.insert(user_id, entry);

        let buckets = BTreeSet::from([DirtyBucket::Inventory]);
        let (first, second) = tokio::join!(
            repository.flush_player_buckets(&user_id, buckets.clone()),
            repository.flush_player_buckets(&user_id, buckets),
        );
        first.expect("first flush");
        second.expect("second flush");

        let (quantity,): (i32,) = sqlx::query_as(
            "SELECT quantity FROM user_inventory WHERE user_id = $1 AND item_id = 'coin'",
        )
        .bind(user_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select inventory");
        assert_eq!(quantity, 150);
    }

    #[tokio::test]
    #[ignore]
    async fn db_settle_game_reloads_cache_and_flushes_dirty_stats() {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
            .expect("connect database");
        let repository = PlayerRepository {
            pool: pool.clone(),
            cache: Arc::new(DashMap::new()),
            flush_immediately: false,
        };
        let winner_id = test_user("000000000093");
        let loser_id = test_user("000000000094");
        for (user_id, username) in [
            (winner_id, "pcm_settle_winner"),
            (loser_id, "pcm_settle_loser"),
        ] {
            sqlx::query(
                "INSERT INTO users (id, username, password_hash, email, skill_rating) \
                 VALUES ($1, $2, '', NULL, 1000) \
                 ON CONFLICT (id) DO UPDATE SET skill_rating = 1000, updated_at = NOW()",
            )
            .bind(user_id.to_string())
            .bind(username)
            .execute(&pool)
            .await
            .expect("upsert test user");
        }
        sqlx::query("DELETE FROM player_stats WHERE user_id IN ($1, $2)")
            .bind(winner_id.to_string())
            .bind(loser_id.to_string())
            .execute(&pool)
            .await
            .expect("reset stats");

        repository
            .get_readonly(&winner_id)
            .await
            .expect("load winner");
        repository
            .get_readonly(&loser_id)
            .await
            .expect("load loser");
        assert!(repository.cache.contains_key(&winner_id));
        assert!(repository.cache.contains_key(&loser_id));

        let settlement_id = format!("db-settle-{}", Utc::now().timestamp_micros());
        repository
            .settle_game(&settlement_id, &winner_id, &loser_id, 25)
            .await
            .expect("settle game");

        let winner_cached = repository.cache.get(&winner_id).expect("winner cached");
        assert_eq!(winner_cached.aggregate.game_info.skill_rating, 1025);
        assert_eq!(winner_cached.aggregate.stats.games_played, 1);
        assert_eq!(winner_cached.aggregate.stats.games_won, 1);
        assert_eq!(winner_cached.aggregate.stats.games_lost, 0);
        assert!(winner_cached.dirty.contains_key(&DirtyBucket::Stats));
        drop(winner_cached);

        let loser_cached = repository.cache.get(&loser_id).expect("loser cached");
        assert_eq!(loser_cached.aggregate.game_info.skill_rating, 975);
        assert_eq!(loser_cached.aggregate.stats.games_played, 1);
        assert_eq!(loser_cached.aggregate.stats.games_won, 0);
        assert_eq!(loser_cached.aggregate.stats.games_lost, 1);
        assert!(loser_cached.dirty.contains_key(&DirtyBucket::Stats));
        drop(loser_cached);

        let (winner_sr,): (i32,) = sqlx::query_as("SELECT skill_rating FROM users WHERE id = $1")
            .bind(winner_id.to_string())
            .fetch_one(&pool)
            .await
            .expect("select winner sr");
        let (loser_sr,): (i32,) = sqlx::query_as("SELECT skill_rating FROM users WHERE id = $1")
            .bind(loser_id.to_string())
            .fetch_one(&pool)
            .await
            .expect("select loser sr");
        assert_eq!(winner_sr, 1025);
        assert_eq!(loser_sr, 975);

        let (winner_played_before,): (i32,) =
            sqlx::query_as("SELECT games_played FROM player_stats WHERE user_id = $1")
                .bind(winner_id.to_string())
                .fetch_one(&pool)
                .await
                .expect("select winner stats before flush");
        assert_eq!(winner_played_before, 0);

        for user_id in [winner_id, loser_id] {
            let mut cached = repository.cache.get_mut(&user_id).expect("cached player");
            let state = cached
                .dirty
                .get_mut(&DirtyBucket::Stats)
                .expect("dirty stats");
            state.changed_at = Utc::now() - ChronoDuration::seconds(FLUSH_COOLDOWN_SECS + 1);
        }

        repository.tick_flush().await.expect("tick flush");

        let (winner_played, winner_won, winner_lost): (i32, i32, i32) = sqlx::query_as(
            "SELECT games_played, games_won, games_lost FROM player_stats WHERE user_id = $1",
        )
        .bind(winner_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select winner stats after flush");
        let (loser_played, loser_won, loser_lost): (i32, i32, i32) = sqlx::query_as(
            "SELECT games_played, games_won, games_lost FROM player_stats WHERE user_id = $1",
        )
        .bind(loser_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select loser stats after flush");
        assert_eq!((winner_played, winner_won, winner_lost), (1, 1, 0));
        assert_eq!((loser_played, loser_won, loser_lost), (1, 0, 1));
        assert!(!repository
            .cache
            .get(&winner_id)
            .expect("winner cached")
            .dirty
            .contains_key(&DirtyBucket::Stats));
        assert!(!repository
            .cache
            .get(&loser_id)
            .expect("loser cached")
            .dirty
            .contains_key(&DirtyBucket::Stats));
    }

    #[tokio::test]
    #[ignore]
    async fn db_claim_story_quest_reward_is_manual_and_idempotent() {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
            .expect("connect database");
        let repository = PlayerRepository {
            pool: pool.clone(),
            cache: Arc::new(DashMap::new()),
            flush_immediately: false,
        };
        let winner_id = test_user("000000000095");
        let loser_id = test_user("000000000096");
        for (user_id, username) in [
            (winner_id, "pcm_claim_winner"),
            (loser_id, "pcm_claim_loser"),
        ] {
            sqlx::query(
                "INSERT INTO users (id, username, password_hash, email, skill_rating) \
                 VALUES ($1, $2, '', NULL, 1000) \
                 ON CONFLICT (id) DO UPDATE SET skill_rating = 1000, updated_at = NOW()",
            )
            .bind(user_id.to_string())
            .bind(username)
            .execute(&pool)
            .await
            .expect("upsert test user");
        }
        for user_id in [winner_id, loser_id] {
            sqlx::query("DELETE FROM asset_ledger WHERE user_id = $1")
                .bind(user_id.to_string())
                .execute(&pool)
                .await
                .expect("clean ledger");
            sqlx::query("DELETE FROM player_operation_idempotency WHERE user_id = $1")
                .bind(user_id.to_string())
                .execute(&pool)
                .await
                .expect("clean idempotency");
            sqlx::query("DELETE FROM user_inventory WHERE user_id = $1")
                .bind(user_id.to_string())
                .execute(&pool)
                .await
                .expect("clean inventory");
            sqlx::query("DELETE FROM user_quest_progress WHERE user_id = $1")
                .bind(user_id.to_string())
                .execute(&pool)
                .await
                .expect("clean quest progress");
            sqlx::query("DELETE FROM user_achievement_progress WHERE user_id = $1")
                .bind(user_id.to_string())
                .execute(&pool)
                .await
                .expect("clean achievement progress");
            sqlx::query("DELETE FROM player_stats WHERE user_id = $1")
                .bind(user_id.to_string())
                .execute(&pool)
                .await
                .expect("clean stats");
        }

        let settlement_id = format!("db-claim-{}", Utc::now().timestamp_micros());
        repository
            .settle_game(&settlement_id, &winner_id, &loser_id, 25)
            .await
            .expect("settle game");

        let (completed_at, rewarded): (Option<DateTime<Utc>>, bool) = sqlx::query_as(
            "SELECT completed_at, rewarded FROM user_quest_progress \
             WHERE user_id = $1 AND quest_id = 'story_first_win' AND quest_date IS NULL",
        )
        .bind(winner_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select story progress");
        assert!(completed_at.is_some());
        assert!(!rewarded);

        let coin_before: Option<(i32,)> = sqlx::query_as(
            "SELECT quantity FROM user_inventory WHERE user_id = $1 AND item_id = 'coin'",
        )
        .bind(winner_id.to_string())
        .fetch_optional(&pool)
        .await
        .expect("select coin before claim");
        assert_eq!(coin_before.map(|row| row.0).unwrap_or(0), 0);

        let reward = repository
            .claim_quest_reward(&winner_id, QuestClaimKind::Story, "story_first_win")
            .await
            .expect("claim story reward");
        assert_eq!(reward.quest_id, "story_first_win");
        assert_eq!(reward.reward_item_id.as_deref(), Some("coin"));
        assert_eq!(reward.reward_quantity, 50);

        let (rewarded_after,): (bool,) = sqlx::query_as(
            "SELECT rewarded FROM user_quest_progress \
             WHERE user_id = $1 AND quest_id = 'story_first_win' AND quest_date IS NULL",
        )
        .bind(winner_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select rewarded after claim");
        assert!(rewarded_after);

        let (coin_after,): (i32,) = sqlx::query_as(
            "SELECT quantity FROM user_inventory WHERE user_id = $1 AND item_id = 'coin'",
        )
        .bind(winner_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select coin after claim");
        assert_eq!(coin_after, 50);

        let duplicate = repository
            .claim_quest_reward(&winner_id, QuestClaimKind::Story, "story_first_win")
            .await;
        assert!(matches!(duplicate, Err(QuestClaimError::AlreadyClaimed)));
        let (coin_after_duplicate,): (i32,) = sqlx::query_as(
            "SELECT quantity FROM user_inventory WHERE user_id = $1 AND item_id = 'coin'",
        )
        .bind(winner_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select coin after duplicate claim");
        assert_eq!(coin_after_duplicate, 50);

        sqlx::query(
            "INSERT INTO user_quest_progress \
             (user_id, quest_id, current_value, completed_at, rewarded) \
             VALUES ($1, 'story_ten_wins', 3, NULL, false) \
             ON CONFLICT (user_id, quest_id, COALESCE(quest_date, '1970-01-01'::date)) \
             DO UPDATE SET current_value = 3, completed_at = NULL, rewarded = false",
        )
        .bind(winner_id.to_string())
        .execute(&pool)
        .await
        .expect("seed incomplete quest");
        let incomplete = repository
            .claim_quest_reward(&winner_id, QuestClaimKind::Story, "story_ten_wins")
            .await;
        assert!(matches!(incomplete, Err(QuestClaimError::NotCompleted)));
    }
}
