use sqlx::PgPool;

use crate::api::chat::{PostedChatMsg, PublicChatMsg};
use crate::api::users::user::UserId;

pub struct ChatMsgCollection {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
struct DbChatMsg {
    id: i64,
    from_id: Option<String>,
    content: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl ChatMsgCollection {
    pub fn new(pool: PgPool) -> Self {
        ChatMsgCollection { pool }
    }

    pub async fn get_messages_in_thread(
        &self,
        thread_id: String,
        maybe_before_id: Option<u64>,
    ) -> Vec<PublicChatMsg> {
        let rows: Vec<DbChatMsg> = if let Some(before_id) = maybe_before_id {
            sqlx::query_as(
                "SELECT id, from_id, content, created_at \
                 FROM   chat_messages \
                 WHERE  thread_id = $1 AND id < $2 \
                 ORDER  BY id DESC \
                 LIMIT  50",
            )
            .bind(&thread_id)
            .bind(before_id as i64)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default()
        } else {
            sqlx::query_as(
                "SELECT id, from_id, content, created_at \
                 FROM   chat_messages \
                 WHERE  thread_id = $1 \
                 ORDER  BY id DESC \
                 LIMIT  50",
            )
            .bind(&thread_id)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default()
        };

        rows.into_iter()
            .map(|r| PublicChatMsg {
                id: r.id,
                from: r.from_id.and_then(|s| UserId::from_str(&s).ok()),
                timestamp: r.created_at.timestamp(),
                content: r.content,
            })
            .collect()
    }

    pub async fn add(
        &self,
        thread_id: String,
        from_id: Option<UserId>,
        msg: PostedChatMsg,
    ) -> Result<(), ()> {
        sqlx::query(
            "INSERT INTO chat_messages (thread_id, from_id, content) VALUES ($1, $2, $3)",
        )
        .bind(&thread_id)
        .bind(from_id.map(|u| u.to_string()))
        .bind(&msg.content)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|_| ())
    }
}


