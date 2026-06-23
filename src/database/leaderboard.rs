use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub struct LeaderboardCollection {
    pool: PgPool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LeaderboardEntry {
    pub rank: i64,
    pub user_id: String,
    pub username: String,
    pub skill_rating: i32,
    pub wins: i64,
}

impl LeaderboardCollection {
    pub fn new(pool: PgPool) -> Self {
        LeaderboardCollection { pool }
    }

    pub async fn get_top_by_rating(&self, limit: i64, offset: i64) -> Vec<LeaderboardEntry> {
        let rows: Vec<(i64, String, String, i32, i64)> = sqlx::query_as(
            "SELECT ROW_NUMBER() OVER (ORDER BY skill_rating DESC) AS rank, \
                    u.id, u.username, u.skill_rating, \
                    COUNT(g.id) AS wins \
             FROM users u \
             LEFT JOIN games g ON g.winner_id = u.id \
             WHERE u.deleted_at IS NULL \
             GROUP BY u.id, u.username, u.skill_rating \
             ORDER BY skill_rating DESC \
             LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .map(
                |(rank, user_id, username, skill_rating, wins)| LeaderboardEntry {
                    rank,
                    user_id,
                    username,
                    skill_rating,
                    wins,
                },
            )
            .collect()
    }

    pub async fn get_top_by_wins(&self, limit: i64, offset: i64) -> Vec<LeaderboardEntry> {
        sqlx::query_as::<_, (i64, String, String, i32, i64)>(
            "SELECT ROW_NUMBER() OVER (ORDER BY COUNT(g.id) DESC) AS rank, \
                    u.id, u.username, u.skill_rating, \
                    COUNT(g.id) AS wins \
             FROM users u \
             LEFT JOIN games g ON g.winner_id = u.id \
             WHERE u.deleted_at IS NULL \
             GROUP BY u.id, u.username, u.skill_rating \
             ORDER BY wins DESC \
             LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(
            |(rank, user_id, username, skill_rating, wins)| LeaderboardEntry {
                rank,
                user_id,
                username,
                skill_rating,
                wins,
            },
        )
        .collect()
    }

    pub async fn get_user_rank(&self, user_id: &str) -> Option<LeaderboardEntry> {
        let row: Option<(i64, String, String, i32, i64)> = sqlx::query_as(
            "SELECT rank, user_id, username, skill_rating, wins FROM ( \
               SELECT ROW_NUMBER() OVER (ORDER BY skill_rating DESC) AS rank, \
                      u.id AS user_id, u.username, u.skill_rating, \
                      COUNT(g.id) AS wins \
               FROM users u \
               LEFT JOIN games g ON g.winner_id = u.id \
               WHERE u.deleted_at IS NULL \
               GROUP BY u.id, u.username, u.skill_rating \
             ) sub WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        row.map(
            |(rank, user_id, username, skill_rating, wins)| LeaderboardEntry {
                rank,
                user_id,
                username,
                skill_rating,
                wins,
            },
        )
    }
}
