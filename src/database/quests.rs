use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::api::users::user::UserId;
use crate::quests::GameEvent;

pub struct QuestCollection {
    pool: PgPool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct QuestProgress {
    pub quest_id: String,
    pub current_value: i32,
    pub completed_at: Option<DateTime<Utc>>,
    pub rewarded: bool,
    pub quest_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct DbQuest {
    id: String,
    condition_value: i32,
    reward_item_id: Option<String>,
    reward_quantity: i32,
}

impl QuestCollection {
    pub fn new(pool: PgPool) -> Self {
        QuestCollection { pool }
    }

    pub async fn on_event(&self, user_id: &UserId, event: &GameEvent) -> Vec<(String, i32)> {
        let mut rewards = Vec::new();
        let condition = event.condition_type();
        let uid = user_id.to_string();
        let today = Utc::now().date_naive();

        let story_quests: Vec<DbQuest> = sqlx::query_as(
            "SELECT id, condition_value, reward_item_id, reward_quantity \
             FROM quests WHERE quest_type = 'story' AND condition_type = $1",
        )
        .bind(condition)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        for quest in &story_quests {
            let maybe_prog: Option<(i32, Option<DateTime<Utc>>, bool)> = sqlx::query_as(
                "SELECT current_value, completed_at, rewarded FROM user_quest_progress \
                 WHERE user_id = $1 AND quest_id = $2 AND quest_date IS NULL",
            )
            .bind(&uid)
            .bind(&quest.id)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();

            match maybe_prog {
                None => {
                    let is_first: Option<(bool,)> = sqlx::query_as(
                        "SELECT NOT EXISTS(SELECT 1 FROM quests WHERE quest_type = 'story' AND next_quest_id = $1)",
                    )
                    .bind(&quest.id)
                    .fetch_one(&self.pool)
                    .await
                    .ok();

                    let prev_done: Option<(bool,)> = sqlx::query_as(
                        "SELECT EXISTS(SELECT 1 FROM user_quest_progress uqp \
                          JOIN quests q ON q.next_quest_id = $1 \
                          WHERE uqp.user_id = $2 AND uqp.quest_id = q.id AND uqp.completed_at IS NOT NULL \
                          AND uqp.quest_date IS NULL)",
                    )
                    .bind(&quest.id)
                    .bind(&uid)
                    .fetch_one(&self.pool)
                    .await
                    .ok();

                    let accessible = is_first.map(|(b,)| b).unwrap_or(false)
                        || prev_done.map(|(b,)| b).unwrap_or(false);

                    if accessible {
                        let new_value = 1_i32;
                        let completed = new_value >= quest.condition_value;
                        let _ = sqlx::query(
                            "INSERT INTO user_quest_progress (user_id, quest_id, current_value, completed_at, rewarded) \
                             VALUES ($1, $2, $3, $4, $5)",
                        )
                        .bind(&uid)
                        .bind(&quest.id)
                        .bind(new_value)
                        .bind(if completed { Some(Utc::now()) } else { None })
                        .bind(false)
                        .execute(&self.pool)
                        .await;

                        if completed {
                            self.mark_rewarded_story(&uid, &quest.id).await;
                            if let Some((item_id, qty)) =
                                reward_tuple(&quest.reward_item_id, quest.reward_quantity)
                            {
                                rewards.push((item_id, qty));
                            }
                        }
                    }
                }
                Some((current_value, None, _)) => {
                    let new_value = current_value + 1;
                    let completed = new_value >= quest.condition_value;
                    let _ = sqlx::query(
                        "UPDATE user_quest_progress SET current_value = $1, completed_at = $2 \
                         WHERE user_id = $3 AND quest_id = $4 AND quest_date IS NULL",
                    )
                    .bind(new_value)
                    .bind(if completed { Some(Utc::now()) } else { None })
                    .bind(&uid)
                    .bind(&quest.id)
                    .execute(&self.pool)
                    .await
                    .ok();

                    if completed {
                        self.mark_rewarded_story(&uid, &quest.id).await;
                        if let Some((item_id, qty)) =
                            reward_tuple(&quest.reward_item_id, quest.reward_quantity)
                        {
                            rewards.push((item_id, qty));
                        }
                    }
                }
                Some((_, Some(_), _)) => {}
            }
        }

        let daily_quests: Vec<DbQuest> = sqlx::query_as(
            "SELECT id, condition_value, reward_item_id, reward_quantity \
             FROM quests WHERE quest_type = 'daily' AND condition_type = $1",
        )
        .bind(condition)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        for quest in &daily_quests {
            let maybe_prog: Option<(i32, Option<DateTime<Utc>>, bool)> = sqlx::query_as(
                "SELECT current_value, completed_at, rewarded FROM user_quest_progress \
                 WHERE user_id = $1 AND quest_id = $2 AND quest_date = $3",
            )
            .bind(&uid)
            .bind(&quest.id)
            .bind(today)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();

            match maybe_prog {
                None => {
                    let new_value = 1_i32;
                    let completed = new_value >= quest.condition_value;
                    let _ = sqlx::query(
                        "INSERT INTO user_quest_progress (user_id, quest_id, current_value, completed_at, rewarded, quest_date) \
                         VALUES ($1, $2, $3, $4, $5, $6) \
                         ON CONFLICT (user_id, quest_id, COALESCE(quest_date, '1970-01-01'::date)) DO NOTHING",
                    )
                    .bind(&uid)
                    .bind(&quest.id)
                    .bind(new_value)
                    .bind(if completed { Some(Utc::now()) } else { None })
                    .bind(false)
                    .bind(today)
                    .execute(&self.pool)
                    .await
                    .ok();

                    if completed {
                        self.mark_rewarded_daily(&uid, &quest.id, today).await;
                        if let Some((item_id, qty)) =
                            reward_tuple(&quest.reward_item_id, quest.reward_quantity)
                        {
                            rewards.push((item_id, qty));
                        }
                    }
                }
                Some((current_value, None, _)) => {
                    let new_value = current_value + 1;
                    let completed = new_value >= quest.condition_value;
                    let _ = sqlx::query(
                        "UPDATE user_quest_progress SET current_value = $1, completed_at = $2 \
                         WHERE user_id = $3 AND quest_id = $4 AND quest_date = $5",
                    )
                    .bind(new_value)
                    .bind(if completed { Some(Utc::now()) } else { None })
                    .bind(&uid)
                    .bind(&quest.id)
                    .bind(today)
                    .execute(&self.pool)
                    .await
                    .ok();

                    if completed {
                        self.mark_rewarded_daily(&uid, &quest.id, today).await;
                        if let Some((item_id, qty)) =
                            reward_tuple(&quest.reward_item_id, quest.reward_quantity)
                        {
                            rewards.push((item_id, qty));
                        }
                    }
                }
                _ => {}
            }
        }

        if matches!(event, GameEvent::GameWon) {
            let achievements: Vec<(String, i32)> = sqlx::query_as::<_, (String, i32)>(
                "SELECT DISTINCT achievement_id, condition_value FROM achievement_tiers \
                 WHERE tier = 1 AND condition_value > 0",
            )
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

            for (achievement_id, _) in &achievements {
                let maybe_prog: Option<(i32, i32)> = sqlx::query_as(
                    "SELECT current_tier, current_value FROM user_achievement_progress \
                     WHERE user_id = $1 AND achievement_id = $2",
                )
                .bind(&uid)
                .bind(achievement_id)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();

                let (current_tier, current_value) = maybe_prog.unwrap_or((1, 0));
                let new_value = current_value + 1;
                let maybe_tier: Option<(i32, Option<String>, i32)> = sqlx::query_as(
                    "SELECT condition_value, reward_item_id, reward_quantity FROM achievement_tiers \
                     WHERE achievement_id = $1 AND tier = $2",
                )
                .bind(achievement_id)
                .bind(current_tier)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();

                if let Some((target, reward_item_id, reward_quantity)) = maybe_tier {
                    if maybe_prog.is_none() {
                        let _ = sqlx::query(
                            "INSERT INTO user_achievement_progress (user_id, achievement_id, current_tier, current_value) \
                             VALUES ($1, $2, $3, $4) ON CONFLICT (user_id, achievement_id) DO NOTHING",
                        )
                        .bind(&uid)
                        .bind(achievement_id)
                        .bind(current_tier)
                        .bind(new_value)
                        .execute(&self.pool)
                        .await;
                    } else {
                        let _ = sqlx::query(
                            "UPDATE user_achievement_progress SET current_value = $1 \
                             WHERE user_id = $2 AND achievement_id = $3",
                        )
                        .bind(new_value)
                        .bind(&uid)
                        .bind(achievement_id)
                        .execute(&self.pool)
                        .await;
                    }

                    if new_value >= target {
                        let _ = sqlx::query(
                            "UPDATE user_achievement_progress SET current_tier = $1, current_value = 0 \
                             WHERE user_id = $2 AND achievement_id = $3",
                        )
                        .bind(current_tier + 1)
                        .bind(&uid)
                        .bind(achievement_id)
                        .execute(&self.pool)
                        .await;

                        if let Some((item_id, qty)) = reward_tuple(&reward_item_id, reward_quantity)
                        {
                            rewards.push((item_id, qty));
                        }
                    }
                }
            }
        }

        rewards
    }

    pub async fn get_story_progress(&self, user_id: &UserId) -> Vec<QuestProgress> {
        sqlx::query_as::<_, QuestProgress>(
            "SELECT uqp.quest_id, uqp.current_value, uqp.completed_at, uqp.rewarded, uqp.quest_date \
             FROM user_quest_progress uqp \
             JOIN quests q ON q.id = uqp.quest_id \
             WHERE uqp.user_id = $1 AND q.quest_type = 'story' AND uqp.quest_date IS NULL",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
    }

    pub async fn get_daily_progress(&self, user_id: &UserId) -> Vec<QuestProgress> {
        let today = Utc::now().date_naive();
        sqlx::query_as::<_, QuestProgress>(
            "SELECT quest_id, current_value, completed_at, rewarded, quest_date \
             FROM user_quest_progress \
             WHERE user_id = $1 AND quest_date = $2",
        )
        .bind(user_id.to_string())
        .bind(today)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
    }

    pub async fn get_achievement_progress(&self, user_id: &UserId) -> Vec<(String, i32, i32)> {
        sqlx::query_as::<_, (String, i32, i32)>(
            "SELECT achievement_id, current_tier, current_value FROM user_achievement_progress \
             WHERE user_id = $1",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
    }

    async fn mark_rewarded_story(&self, user_id: &str, quest_id: &str) {
        let _ = sqlx::query(
            "UPDATE user_quest_progress SET rewarded = true \
             WHERE user_id = $1 AND quest_id = $2 AND quest_date IS NULL",
        )
        .bind(user_id)
        .bind(quest_id)
        .execute(&self.pool)
        .await;
    }

    async fn mark_rewarded_daily(&self, user_id: &str, quest_id: &str, quest_date: NaiveDate) {
        let _ = sqlx::query(
            "UPDATE user_quest_progress SET rewarded = true \
             WHERE user_id = $1 AND quest_id = $2 AND quest_date = $3",
        )
        .bind(user_id)
        .bind(quest_id)
        .bind(quest_date)
        .execute(&self.pool)
        .await;
    }
}

fn reward_tuple(reward_item_id: &Option<String>, reward_quantity: i32) -> Option<(String, i32)> {
    reward_item_id
        .as_ref()
        .filter(|_| reward_quantity > 0)
        .map(|item_id| (item_id.clone(), reward_quantity))
}
