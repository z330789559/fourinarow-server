use sqlx::PgPool;

use crate::api::users::user::PlayedGameInfo;

pub struct GameCollection {
    pool: PgPool,
}

impl GameCollection {
    pub fn new(pool: PgPool) -> Self {
        GameCollection { pool }
    }

    pub async fn insert(&self, game: PlayedGameInfo) -> bool {
        sqlx::query(
            "INSERT INTO games (winner_id, loser_id) VALUES ($1, $2)",
        )
        .bind(game.winner.to_string())
        .bind(game.loser.to_string())
        .execute(&self.pool)
        .await
        .is_ok()
    }
}

