use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::api::users::user::UserId;

pub struct InviteCollection {
    pool: PgPool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteCode {
    pub code: String,
    pub creator_id: String,
    pub max_uses: i32,
    pub uses: i32,
    pub reward_item_id: Option<String>,
    pub reward_quantity: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl InviteCollection {
    pub fn new(pool: PgPool) -> Self {
        InviteCollection { pool }
    }

    pub async fn create(&self, creator_id: &UserId, max_uses: i32) -> Option<InviteCode> {
        let code: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(12)
            .map(char::from)
            .collect::<String>()
            .to_uppercase();

        sqlx::query("INSERT INTO invite_codes (code, creator_id, max_uses) VALUES ($1, $2, $3)")
            .bind(&code)
            .bind(creator_id.to_string())
            .bind(max_uses)
            .execute(&self.pool)
            .await
            .ok()?;

        self.get(&code).await
    }

    pub async fn get(&self, code: &str) -> Option<InviteCode> {
        let row: Option<(
            String,
            String,
            i32,
            i32,
            Option<String>,
            i32,
            chrono::DateTime<chrono::Utc>,
            Option<chrono::DateTime<chrono::Utc>>,
        )> = sqlx::query_as(
            "SELECT code, creator_id, max_uses, uses, reward_item_id, reward_quantity, created_at, expires_at \
             FROM invite_codes WHERE code = $1",
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        row.map(
            |(
                code,
                creator_id,
                max_uses,
                uses,
                reward_item_id,
                reward_quantity,
                created_at,
                expires_at,
            )| InviteCode {
                code,
                creator_id,
                max_uses,
                uses,
                reward_item_id,
                reward_quantity,
                created_at,
                expires_at,
            },
        )
    }

    async fn redeem(&self, code: &str, user_id: &UserId) -> Result<Option<(String, i32)>, String> {
        let invite = self
            .get(code)
            .await
            .ok_or_else(|| "Invalid invite code".to_string())?;

        if let Some(expires_at) = invite.expires_at {
            if chrono::Utc::now() > expires_at {
                return Err("Invite code has expired".to_string());
            }
        }

        if invite.uses >= invite.max_uses {
            return Err("Invite code has reached its maximum uses".to_string());
        }

        let already_used: Option<(bool,)> = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM invite_code_uses WHERE code = $1 AND used_by = $2)",
        )
        .bind(code)
        .bind(user_id.to_string())
        .fetch_one(&self.pool)
        .await
        .ok();

        if already_used.map(|(used,)| used).unwrap_or(false) {
            return Err("You have already used this invite code".to_string());
        }

        sqlx::query("INSERT INTO invite_code_uses (code, used_by) VALUES ($1, $2)")
            .bind(code)
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|error| format!("DB error: {error}"))?;

        sqlx::query("UPDATE invite_codes SET uses = uses + 1 WHERE code = $1")
            .bind(code)
            .execute(&self.pool)
            .await
            .map_err(|error| format!("DB error: {error}"))?;

        Ok(invite
            .reward_item_id
            .filter(|_| invite.reward_quantity > 0)
            .map(|item_id| (item_id, invite.reward_quantity)))
    }

    pub async fn list_by_creator(&self, creator_id: &UserId) -> Vec<InviteCode> {
        let rows: Vec<(
            String,
            String,
            i32,
            i32,
            Option<String>,
            i32,
            chrono::DateTime<chrono::Utc>,
            Option<chrono::DateTime<chrono::Utc>>,
        )> = sqlx::query_as(
            "SELECT code, creator_id, max_uses, uses, reward_item_id, reward_quantity, created_at, expires_at \
             FROM invite_codes WHERE creator_id = $1 ORDER BY created_at DESC",
        )
        .bind(creator_id.to_string())
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .map(
                |(
                    code,
                    creator_id,
                    max_uses,
                    uses,
                    reward_item_id,
                    reward_quantity,
                    created_at,
                    expires_at,
                )| InviteCode {
                    code,
                    creator_id,
                    max_uses,
                    uses,
                    reward_item_id,
                    reward_quantity,
                    created_at,
                    expires_at,
                },
            )
            .collect()
    }
}
