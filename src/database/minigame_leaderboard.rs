//! 小游戏排行榜数据层（OpenSpec: minigame-leaderboard-service）。
//!
//! 每关只留最佳分/星；排行榜按 SUM(best_score) 实时聚合（并列以 SUM(best_stars)、
//! 最早达成时间决胜）。写法风格对齐既有 `database::leaderboard`。

use serde::Serialize;
use sqlx::PgPool;

pub struct MinigameLeaderboardCollection {
    pool: PgPool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MinigameBoardEntry {
    pub rank: i64,
    pub user_id: String,
    pub username: String,
    pub total_score: i64,
    pub total_stars: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MinigameMyRank {
    pub rank: i64,
    pub username: String,
    pub total_score: i64,
    pub total_stars: i64,
}

impl MinigameLeaderboardCollection {
    pub fn new(pool: PgPool) -> Self {
        MinigameLeaderboardCollection { pool }
    }

    /// upsert 每关最佳；返回该关最终 (best_score, best_stars)。
    pub async fn submit_score(
        &self,
        user_id: &str,
        game_key: &str,
        level_id: i32,
        score: i32,
        stars: i16,
    ) -> Result<(i32, i16), sqlx::Error> {
        let row: (i32, i16) = sqlx::query_as(
            "INSERT INTO minigame_level_score \
                 (user_id, game_key, level_id, best_score, best_stars, updated_at) \
             VALUES ($1, $2, $3, $4, $5, now()) \
             ON CONFLICT (user_id, game_key, level_id) DO UPDATE SET \
                 best_score = GREATEST(minigame_level_score.best_score, EXCLUDED.best_score), \
                 best_stars = GREATEST(minigame_level_score.best_stars, EXCLUDED.best_stars), \
                 updated_at = now() \
             RETURNING best_score, best_stars",
        )
        .bind(user_id)
        .bind(game_key)
        .bind(level_id)
        .bind(score)
        .bind(stars)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Top 榜（分页）。
    pub async fn get_top(&self, game_key: &str, limit: i64, offset: i64) -> Vec<MinigameBoardEntry> {
        let rows: Vec<(i64, String, String, i64, i64)> = sqlx::query_as(
            "SELECT ROW_NUMBER() OVER ( \
                        ORDER BY s.total_score DESC, s.total_stars DESC, s.first_at ASC \
                    ) AS rank, \
                    s.user_id, u.username, s.total_score, s.total_stars \
             FROM ( \
               SELECT user_id, \
                      SUM(best_score)::bigint AS total_score, \
                      SUM(best_stars)::bigint AS total_stars, \
                      MIN(updated_at) AS first_at \
               FROM minigame_level_score \
               WHERE game_key = $1 \
               GROUP BY user_id \
             ) s \
             JOIN users u ON u.id = s.user_id AND u.deleted_at IS NULL \
             ORDER BY rank \
             LIMIT $2 OFFSET $3",
        )
        .bind(game_key)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .map(
                |(rank, user_id, username, total_score, total_stars)| MinigameBoardEntry {
                    rank,
                    user_id,
                    username,
                    total_score,
                    total_stars,
                },
            )
            .collect()
    }

    /// 某玩家的名次与总分（口径与 get_top 一致）；无成绩返回 None。
    pub async fn get_user_rank(&self, game_key: &str, user_id: &str) -> Option<MinigameMyRank> {
        let row: Option<(i64, String, String, i64, i64)> = sqlx::query_as(
            "SELECT rank, user_id, username, total_score, total_stars FROM ( \
               SELECT ROW_NUMBER() OVER ( \
                          ORDER BY s.total_score DESC, s.total_stars DESC, s.first_at ASC \
                      ) AS rank, \
                      s.user_id, u.username, s.total_score, s.total_stars \
               FROM ( \
                 SELECT user_id, \
                        SUM(best_score)::bigint AS total_score, \
                        SUM(best_stars)::bigint AS total_stars, \
                        MIN(updated_at) AS first_at \
                 FROM minigame_level_score \
                 WHERE game_key = $1 \
                 GROUP BY user_id \
               ) s \
               JOIN users u ON u.id = s.user_id AND u.deleted_at IS NULL \
             ) ranked \
             WHERE user_id = $2",
        )
        .bind(game_key)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        row.map(
            |(rank, _user_id, username, total_score, total_stars)| MinigameMyRank {
                rank,
                username,
                total_score,
                total_stars,
            },
        )
    }
}
