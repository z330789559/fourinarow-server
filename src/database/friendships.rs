use sqlx::PgPool;

use crate::api::{
    chat::ChatThreadId,
    users::user::{BackendFriendshipMe, BackendFriendshipState, BackendFriendshipsMe, UserId},
};

pub struct FriendshipCollection {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
struct DbFriendship {
    user_id_1: String,
    user_id_2: String,
    requester_id: String,
    status: String,
    chat_thread_id: Option<String>,
}

impl DbFriendship {
    fn to_backend(self, viewer_id: UserId) -> Option<BackendFriendshipMe> {
        let viewer_str = viewer_id.to_string();
        let other_str = if self.user_id_1 == viewer_str {
            &self.user_id_2
        } else {
            &self.user_id_1
        };
        let other_id = UserId::from_str(other_str).ok()?;

        let state = if self.status == "friends" {
            BackendFriendshipState::Friends {
                chat_thread_id: self
                    .chat_thread_id
                    .unwrap_or_default()
                    .as_str()
                    .into(),
            }
        } else if self.requester_id == viewer_str {
            BackendFriendshipState::ReqOutgoing
        } else {
            BackendFriendshipState::ReqIncoming
        };

        Some(BackendFriendshipMe { other_id, state })
    }
}

/// Normalise a friendship pair so that user_id_1 < user_id_2 lexicographically.
fn normalize(a: &UserId, b: &UserId) -> (String, String) {
    let (sa, sb) = (a.to_string(), b.to_string());
    if sa < sb {
        (sa, sb)
    } else {
        (sb, sa)
    }
}

impl FriendshipCollection {
    pub fn new(pool: PgPool) -> Self {
        FriendshipCollection { pool }
    }

    pub async fn get_for(&self, user_id: UserId) -> BackendFriendshipsMe {
        let uid = user_id.to_string();
        let rows: Vec<DbFriendship> = sqlx::query_as(
            "SELECT user_id_1, user_id_2, requester_id, status, chat_thread_id \
             FROM   friendships \
             WHERE  user_id_1 = $1 OR user_id_2 = $1",
        )
        .bind(&uid)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        BackendFriendshipsMe::from(
            rows.into_iter()
                .filter_map(|r| r.to_backend(user_id))
                .collect(),
        )
    }

    pub async fn insert(&self, from_id: UserId, to_id: UserId) -> bool {
        let (id1, id2) = normalize(&from_id, &to_id);
        sqlx::query(
            "INSERT INTO friendships (user_id_1, user_id_2, requester_id) \
             VALUES ($1, $2, $3) \
             ON CONFLICT DO NOTHING",
        )
        .bind(&id1)
        .bind(&id2)
        .bind(from_id.to_string())
        .execute(&self.pool)
        .await
        .is_ok()
    }

    pub async fn upgrade_to_friends(
        &self,
        from_id: UserId,
        to_id: UserId,
        chat_thread_id: ChatThreadId,
    ) -> bool {
        let (id1, id2) = normalize(&from_id, &to_id);
        sqlx::query(
            "UPDATE friendships \
             SET    status = 'friends', chat_thread_id = $1, updated_at = NOW() \
             WHERE  user_id_1 = $2 AND user_id_2 = $3",
        )
        .bind(chat_thread_id.to_string())
        .bind(&id1)
        .bind(&id2)
        .execute(&self.pool)
        .await
        .is_ok()
    }

    pub async fn remove(&self, from_id: UserId, to_id: UserId) -> bool {
        let (id1, id2) = normalize(&from_id, &to_id);
        sqlx::query(
            "DELETE FROM friendships WHERE user_id_1 = $1 AND user_id_2 = $2",
        )
        .bind(&id1)
        .bind(&id2)
        .execute(&self.pool)
        .await
        .is_ok()
    }
}

