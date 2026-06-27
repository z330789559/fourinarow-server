use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::api::users::user::UserId;

#[derive(Debug, Clone, Serialize)]
pub struct InboxMessage {
    pub id: i64,
    pub r#type: String,
    pub title: String,
    pub body: String,
    pub reward_item_id: Option<String>,
    pub reward_qty: i32,
    pub claimed: bool,
    pub read: bool,
    pub created_at: DateTime<Utc>,
}

pub struct NewInboxMessage<'a> {
    pub user_id: &'a UserId,
    pub msg_type: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub reward_item_id: Option<&'a str>,
    pub reward_qty: i32,
}

pub async fn insert_inbox_message(
    pool: &PgPool,
    msg: NewInboxMessage<'_>,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO user_inbox (user_id, type, title, body, reward_item_id, reward_qty) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(msg.user_id.to_string())
    .bind(msg.msg_type)
    .bind(msg.title)
    .bind(msg.body)
    .bind(msg.reward_item_id)
    .bind(msg.reward_qty)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn list_inbox(pool: &PgPool, user_id: &UserId) -> Vec<InboxMessage> {
    let rows: Vec<(i64, String, String, String, Option<String>, i32, bool, bool, DateTime<Utc>)> =
        sqlx::query_as(
            "SELECT id, type, title, body, reward_item_id, reward_qty, claimed, read, created_at \
             FROM user_inbox WHERE user_id = $1 ORDER BY created_at DESC",
        )
        .bind(user_id.to_string())
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    rows.into_iter()
        .map(|(id, r#type, title, body, reward_item_id, reward_qty, claimed, read, created_at)| {
            InboxMessage { id, r#type, title, body, reward_item_id, reward_qty, claimed, read, created_at }
        })
        .collect()
}

pub enum ClaimError {
    NotFound,
    AlreadyClaimed,
    NoReward,
    Db(sqlx::Error),
}

pub async fn claim_inbox(
    pool: &PgPool,
    user_id: &UserId,
    inbox_id: i64,
) -> Result<(String, i32), ClaimError> {
    let mut tx = pool.begin().await.map_err(ClaimError::Db)?;

    let row: Option<(Option<String>, i32, bool)> = sqlx::query_as(
        "SELECT reward_item_id, reward_qty, claimed FROM user_inbox \
         WHERE id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(inbox_id)
    .bind(user_id.to_string())
    .fetch_optional(&mut *tx)
    .await
    .map_err(ClaimError::Db)?;

    let (item_id_opt, qty, claimed) = row.ok_or(ClaimError::NotFound)?;
    if claimed {
        return Err(ClaimError::AlreadyClaimed);
    }
    let item_id = item_id_opt.ok_or(ClaimError::NoReward)?;
    if qty <= 0 {
        return Err(ClaimError::NoReward);
    }

    let idempotency_key = format!("inbox_claim:{inbox_id}");

    let update_result = sqlx::query(
        "UPDATE user_inbox SET claimed = true WHERE id = $1 AND user_id = $2 AND claimed = false",
    )
    .bind(inbox_id)
    .bind(user_id.to_string())
    .execute(&mut *tx)
    .await
    .map_err(ClaimError::Db)?;
    if update_result.rows_affected() == 0 {
        return Err(ClaimError::AlreadyClaimed);
    }

    // asset_ledger
    let ledger_result = sqlx::query(
        "INSERT INTO asset_ledger (user_id, item_id, delta, source, idempotency_key, business_id) \
         VALUES ($1, $2, $3, 'inbox_claim', $4, $4) \
         ON CONFLICT (idempotency_key, item_id, delta) DO NOTHING",
    )
    .bind(user_id.to_string())
    .bind(&item_id)
    .bind(qty)
    .bind(&idempotency_key)
    .execute(&mut *tx)
    .await
    .map_err(ClaimError::Db)?;
    if ledger_result.rows_affected() == 0 {
        return Err(ClaimError::AlreadyClaimed);
    }

    // add to inventory
    sqlx::query(
        "INSERT INTO user_inventory (user_id, item_id, quantity) VALUES ($1, $2, $3) \
         ON CONFLICT (user_id, item_id) DO UPDATE SET quantity = user_inventory.quantity + $3",
    )
    .bind(user_id.to_string())
    .bind(&item_id)
    .bind(qty)
    .execute(&mut *tx)
    .await
    .map_err(ClaimError::Db)?;

    tx.commit().await.map_err(ClaimError::Db)?;
    Ok((item_id, qty))
}

pub async fn mark_read(pool: &PgPool, user_id: &UserId, inbox_id: i64) -> bool {
    sqlx::query(
        "UPDATE user_inbox SET read = true WHERE id = $1 AND user_id = $2",
    )
    .bind(inbox_id)
    .bind(user_id.to_string())
    .execute(pool)
    .await
    .map(|r| r.rows_affected() > 0)
    .unwrap_or(false)
}

pub async fn delete_inbox(pool: &PgPool, user_id: &UserId, inbox_id: i64) -> Result<(), &'static str> {
    let row: Option<(bool, bool)> = sqlx::query_as(
        "SELECT claimed, reward_item_id IS NOT NULL FROM user_inbox WHERE id = $1 AND user_id = $2",
    )
    .bind(inbox_id)
    .bind(user_id.to_string())
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    match row {
        None => Err("not_found"),
        Some((false, true)) => Err("unclaimed_reward"),
        _ => {
            let _ = sqlx::query("DELETE FROM user_inbox WHERE id = $1 AND user_id = $2")
                .bind(inbox_id)
                .bind(user_id.to_string())
                .execute(pool)
                .await;
            Ok(())
        }
    }
}
