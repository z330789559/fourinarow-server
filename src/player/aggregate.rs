use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::api::users::user::{UserGameInfo, UserId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DirtyBucket {
    Profile,
    GameInfo,
    Inventory,
    Quests,
    Achievements,
    Stats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerProfile {
    pub id: UserId,
    pub username: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStats {
    pub games_played: i32,
    pub games_won: i32,
    pub games_lost: i32,
    pub version: i64,
}

impl Default for PlayerStats {
    fn default() -> Self {
        PlayerStats {
            games_played: 0,
            games_won: 0,
            games_lost: 0,
            version: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerQuestProgress {
    pub quest_id: String,
    pub current_value: i32,
    pub completed_at: Option<DateTime<Utc>>,
    pub rewarded: bool,
    pub quest_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerAchievementProgress {
    pub achievement_id: String,
    pub current_tier: i32,
    pub current_value: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerAggregate {
    pub profile: PlayerProfile,
    pub game_info: UserGameInfo,
    pub inventory: BTreeMap<String, i32>,
    pub quests: Vec<PlayerQuestProgress>,
    pub achievements: Vec<PlayerAchievementProgress>,
    pub stats: PlayerStats,
}

#[derive(Debug, Clone)]
pub struct DirtyBucketState {
    pub changed_at: DateTime<Utc>,
    pub last_flush_at: DateTime<Utc>,
    pub attempts: u32,
    pub reason: Option<String>,
}

impl DirtyBucketState {
    pub fn new(now: DateTime<Utc>, reason: Option<String>) -> Self {
        DirtyBucketState {
            changed_at: now,
            last_flush_at: now,
            attempts: 0,
            reason,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayerCacheEntry {
    pub aggregate: PlayerAggregate,
    pub dirty: BTreeMap<DirtyBucket, DirtyBucketState>,
    pub cached_at: DateTime<Utc>,
}

impl PlayerCacheEntry {
    pub fn clean(aggregate: PlayerAggregate) -> Self {
        PlayerCacheEntry {
            aggregate,
            dirty: BTreeMap::new(),
            cached_at: Utc::now(),
        }
    }

    pub fn mark_dirty<I>(&mut self, buckets: I, reason: Option<&str>)
    where
        I: IntoIterator<Item = DirtyBucket>,
    {
        let now = Utc::now();
        for bucket in buckets {
            self.dirty
                .entry(bucket)
                .and_modify(|state| {
                    state.changed_at = now;
                    state.reason = reason.map(str::to_string);
                })
                .or_insert_with(|| DirtyBucketState::new(now, reason.map(str::to_string)));
        }
    }

    pub fn dirty_buckets(&self) -> BTreeSet<DirtyBucket> {
        self.dirty.keys().copied().collect()
    }

    pub fn clear_bucket(&mut self, bucket: DirtyBucket) {
        self.dirty.remove(&bucket);
    }

    pub fn mark_bucket_flushed(&mut self, bucket: DirtyBucket) {
        self.clear_bucket(bucket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_aggregate() -> PlayerAggregate {
        let user_id = UserId::from_str("000000000001").expect("valid user id");
        PlayerAggregate {
            profile: PlayerProfile {
                id: user_id,
                username: "tester".to_string(),
                email: None,
            },
            game_info: UserGameInfo { skill_rating: 1000 },
            inventory: BTreeMap::new(),
            quests: Vec::new(),
            achievements: Vec::new(),
            stats: PlayerStats::default(),
        }
    }

    #[test]
    fn clean_cache_entry_has_no_dirty_buckets() {
        let entry = PlayerCacheEntry::clean(test_aggregate());

        assert!(entry.dirty.is_empty());
        assert!(entry.dirty_buckets().is_empty());
    }

    #[test]
    fn mark_dirty_tracks_only_selected_buckets() {
        let mut entry = PlayerCacheEntry::clean(test_aggregate());

        entry.mark_dirty([DirtyBucket::Inventory], Some("unit-test"));

        assert!(entry.dirty.contains_key(&DirtyBucket::Inventory));
        assert!(!entry.dirty.contains_key(&DirtyBucket::Profile));
        assert!(!entry.dirty.contains_key(&DirtyBucket::GameInfo));
        assert_eq!(
            entry
                .dirty
                .get(&DirtyBucket::Inventory)
                .unwrap()
                .reason
                .as_deref(),
            Some("unit-test")
        );
    }

    #[test]
    fn failed_flush_can_keep_dirty_and_increment_attempts() {
        let mut entry = PlayerCacheEntry::clean(test_aggregate());
        entry.mark_dirty([DirtyBucket::Stats], Some("flush-failure"));

        entry.dirty.get_mut(&DirtyBucket::Stats).unwrap().attempts += 1;

        let state = entry.dirty.get(&DirtyBucket::Stats).unwrap();
        assert_eq!(state.attempts, 1);
        assert_eq!(state.reason.as_deref(), Some("flush-failure"));
    }

    #[test]
    fn clear_bucket_removes_only_flushed_bucket() {
        let mut entry = PlayerCacheEntry::clean(test_aggregate());
        entry.mark_dirty(
            [DirtyBucket::Inventory, DirtyBucket::Stats],
            Some("multi-bucket"),
        );

        entry.clear_bucket(DirtyBucket::Inventory);

        assert!(!entry.dirty.contains_key(&DirtyBucket::Inventory));
        assert!(entry.dirty.contains_key(&DirtyBucket::Stats));
    }
}
