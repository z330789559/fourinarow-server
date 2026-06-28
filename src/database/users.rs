use actix::Addr;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use sqlx::PgPool;

use super::friendships::FriendshipCollection;
use crate::{
    api::users::{
        session_token::SessionToken,
        user::{
            BackendFriendshipsMe, BackendUserMe, HashedPassword, PublicUserOther, UserGameInfo,
            UserId,
        },
        user_mgr::UserAuth,
    },
    game::client_state::ClientState,
};

const CACHE_TTL_SECS: i64 = 60;

struct CachedEntry {
    username: String,
    password: HashedPassword,
    email: Option<String>,
    game_info: UserGameInfo,
    friendships: BackendFriendshipsMe,
    cached_at: DateTime<Utc>,
}

pub struct UserCollection {
    pool: PgPool,
    pub(super) playing_users_cache: DashMap<UserId, Addr<ClientState>>,
    user_cache: DashMap<UserId, CachedEntry>,
}

#[derive(sqlx::FromRow)]
struct DbUser {
    id: String,
    username: String,
    password_hash: String,
    email: Option<String>,
    skill_rating: i32,
}

impl DbUser {
    fn into_backend(
        self,
        playing: Option<Addr<ClientState>>,
        friendships: BackendFriendshipsMe,
    ) -> Option<BackendUserMe> {
        let user_id = UserId::from_str(&self.id)
            .map_err(|error| log::warn!("Failed to parse user id '{}': {:?}", self.id, error))
            .ok()?;
        let password = HashedPassword::from_str(&self.password_hash)
            .map_err(|error| {
                log::warn!(
                    "Failed to parse password hash for user '{}': {:?}",
                    self.id,
                    error
                )
            })
            .ok()?;

        Some(BackendUserMe {
            id: user_id,
            username: self.username,
            password,
            email: self.email,
            game_info: UserGameInfo {
                skill_rating: self.skill_rating,
            },
            playing,
            friendships,
        })
    }
}

impl UserCollection {
    pub fn new(pool: PgPool) -> Self {
        UserCollection {
            pool,
            playing_users_cache: DashMap::new(),
            user_cache: DashMap::new(),
        }
    }

    fn cache_get(&self, id: &UserId) -> Option<BackendUserMe> {
        let entry = self.user_cache.get(id)?;
        if (Utc::now() - entry.cached_at).num_seconds() >= CACHE_TTL_SECS {
            drop(entry);
            self.user_cache.remove(id);
            return None;
        }

        let playing = self.playing_users_cache.get(id).map(|p| p.clone());
        Some(BackendUserMe {
            id: *id,
            username: entry.username.clone(),
            password: entry.password.clone(),
            email: entry.email.clone(),
            game_info: entry.game_info.clone(),
            playing,
            friendships: entry.friendships.clone(),
        })
    }

    fn cache_put(&self, user: &BackendUserMe) {
        self.user_cache.insert(
            user.id,
            CachedEntry {
                username: user.username.clone(),
                password: user.password.clone(),
                email: user.email.clone(),
                game_info: user.game_info.clone(),
                friendships: user.friendships.clone(),
                cached_at: Utc::now(),
            },
        );
    }

    pub fn invalidate_cache(&self, id: &UserId) {
        self.user_cache.remove(id);
    }

    async fn get_auth(
        &self,
        auth: &UserAuth,
        friendships: &FriendshipCollection,
    ) -> Option<BackendUserMe> {
        if let Some(user) = self.get_username(&auth.username, friendships).await {
            if user.password.matches(&auth.password) {
                return Some(user);
            }
        }
        None
    }

    pub async fn get_session_token(
        &self,
        session_token: SessionToken,
        friendships: &FriendshipCollection,
    ) -> Option<BackendUserMe> {
        let row: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT u.id
            FROM   users    u
            JOIN   sessions s ON u.id = s.user_id
            WHERE  s.token = $1
              AND  u.deleted_at IS NULL
            "#,
        )
        .bind(session_token.to_string())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let user_id = UserId::from_str(&row?.0).ok()?;
        self.get_by_id(&user_id, true, friendships).await
    }

    /// Returns `(token, user_id_string)` on success.
    pub async fn create_session_token(
        &self,
        auth: UserAuth,
        friendships: &FriendshipCollection,
    ) -> Option<(SessionToken, String)> {
        self.get_auth(&auth, friendships).await?;
        let uid: (String,) = sqlx::query_as(
            "SELECT id FROM users WHERE LOWER(username) = LOWER($1) AND deleted_at IS NULL",
        )
        .bind(&auth.username)
        .fetch_one(&self.pool)
        .await
        .ok()?;

        let session_token = SessionToken::new();
        sqlx::query("INSERT INTO sessions (token, user_id) VALUES ($1, $2)")
            .bind(session_token.to_string())
            .bind(&uid.0)
            .execute(&self.pool)
            .await
            .ok()?;

        Some((session_token, uid.0))
    }

    pub async fn remove_session_token(&self, session_token: SessionToken) -> Result<(), ()> {
        sqlx::query("DELETE FROM sessions WHERE token = $1")
            .bind(session_token.to_string())
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|_| ())
    }

    pub async fn get_username(
        &self,
        username: &str,
        friendships: &FriendshipCollection,
    ) -> Option<BackendUserMe> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM users WHERE LOWER(username) = LOWER($1) AND deleted_at IS NULL",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let user_id = UserId::from_str(&row?.0).ok()?;
        self.get_by_id(&user_id, true, friendships).await
    }

    pub async fn get_id(
        &self,
        id: &UserId,
        friendships: &FriendshipCollection,
    ) -> Option<BackendUserMe> {
        self.get_by_id(id, true, friendships).await
    }

    pub async fn get_by_id(
        &self,
        id: &UserId,
        use_cache: bool,
        friendships: &FriendshipCollection,
    ) -> Option<BackendUserMe> {
        if use_cache {
            if let Some(user) = self.cache_get(id) {
                return Some(user);
            }
        }

        let row = sqlx::query_as::<_, DbUser>(
            "SELECT id, username, password_hash, email, skill_rating FROM users \
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()?;

        let user_id = UserId::from_str(&row.id).ok()?;
        let playing = self.playing_users_cache.get(&user_id).map(|p| p.clone());
        let backend_friendships = friendships.get_for(user_id).await;
        let user = row.into_backend(playing, backend_friendships)?;
        self.cache_put(&user);
        Some(user)
    }

    pub async fn get(
        &self,
        id: &UserId,
        use_cache: bool,
        friendships: &FriendshipCollection,
    ) -> Option<BackendUserMe> {
        self.get_by_id(id, use_cache, friendships).await
    }

    pub async fn get_id_public(&self, id: UserId) -> Option<PublicUserOther> {
        let row: (String, String, i32) = sqlx::query_as(
            "SELECT id, username, skill_rating FROM users WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()?;

        let user_id = UserId::from_str(&row.0).ok()?;
        Some(PublicUserOther {
            id: user_id,
            username: row.1,
            game_info: UserGameInfo {
                skill_rating: row.2,
            },
            playing: self.playing_users_cache.contains_key(&user_id),
        })
    }

    pub async fn query(&self, query: &str) -> Vec<PublicUserOther> {
        let pattern = format!("%{}%", query.to_lowercase());
        let rows: Vec<(String, String, i32)> = sqlx::query_as(
            "SELECT id, username, skill_rating FROM users \
             WHERE LOWER(username) LIKE $1 AND deleted_at IS NULL \
             LIMIT 50",
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .filter_map(|(id_str, username, skill_rating)| {
                UserId::from_str(&id_str).ok().map(|uid| PublicUserOther {
                    id: uid,
                    username,
                    game_info: UserGameInfo { skill_rating },
                    playing: self.playing_users_cache.contains_key(&uid),
                })
            })
            .collect()
    }

    pub async fn insert(&self, user: BackendUserMe) -> bool {
        let ok = sqlx::query(
            "INSERT INTO users (id, username, password_hash, email, skill_rating) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(user.id.to_string())
        .bind(&user.username)
        .bind(user.password.to_string())
        .bind(&user.email)
        .bind(user.game_info.skill_rating)
        .execute(&self.pool)
        .await
        .is_ok();

        if ok {
            self.cache_put(&user);
        }
        ok
    }

    pub async fn update(&self, user: BackendUserMe) -> bool {
        if let Some(playing_addr) = &user.playing {
            self.playing_users_cache
                .insert(user.id, playing_addr.clone());
        } else {
            self.playing_users_cache.remove(&user.id);
        }

        self.invalidate_cache(&user.id);
        true
    }

    pub async fn find_or_create_platform_user(
        &self,
        provider: &str,
        provider_user_id: &str,
        union_id: Option<&str>,
        nickname: Option<&str>,
        session_key: Option<&str>,
    ) -> Option<(UserId, SessionToken)> {
        let maybe_uid: Option<(String,)> = sqlx::query_as(
            "SELECT user_id FROM auth_identities WHERE provider = $1 AND provider_user_id = $2",
        )
        .bind(provider)
        .bind(provider_user_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let user_id_str: String = if let Some((uid,)) = maybe_uid {
            if let Some(sk) = session_key {
                let _ = sqlx::query(
                    "UPDATE auth_identities SET session_key = $1, updated_at = NOW() \
                     WHERE provider = $2 AND provider_user_id = $3",
                )
                .bind(sk)
                .bind(provider)
                .bind(provider_user_id)
                .execute(&self.pool)
                .await;
            }
            uid
        } else {
            let mut new_uid = UserId::new();
            let mut id_attempts = 0u8;
            loop {
                let exists: Option<(bool,)> =
                    sqlx::query_as("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
                        .bind(new_uid.to_string())
                        .fetch_one(&self.pool)
                        .await
                        .ok();
                if !exists.map(|(value,)| value).unwrap_or(true) {
                    break;
                }
                id_attempts += 1;
                if id_attempts >= 10 {
                    log::error!("Failed to generate unique user ID after 10 attempts");
                    return None;
                }
                new_uid = UserId::new();
            }

            let base_name = nickname
                .map(|name| name.chars().take(20).collect::<String>())
                .unwrap_or_else(|| {
                    format!(
                        "user_{}",
                        new_uid.to_string().chars().take(6).collect::<String>()
                    )
                });
            let final_name = self.make_unique_username(&base_name).await;

            sqlx::query(
                "INSERT INTO users (id, username, password_hash, email, skill_rating, source_platform) \
                 VALUES ($1, $2, '', NULL, 1000, $3)",
            )
            .bind(new_uid.to_string())
            .bind(&final_name)
            .bind(provider)
            .execute(&self.pool)
            .await
            .ok()?;

            sqlx::query(
                "INSERT INTO auth_identities \
                 (user_id, provider, provider_user_id, union_id, session_key) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(new_uid.to_string())
            .bind(provider)
            .bind(provider_user_id)
            .bind(union_id)
            .bind(session_key)
            .execute(&self.pool)
            .await
            .ok()?;

            new_uid.to_string()
        };

        let user_id = UserId::from_str(&user_id_str).ok()?;
        let session_token = SessionToken::new();
        sqlx::query("INSERT INTO sessions (token, user_id) VALUES ($1, $2)")
            .bind(session_token.to_string())
            .bind(&user_id_str)
            .execute(&self.pool)
            .await
            .ok()?;

        self.invalidate_cache(&user_id);
        Some((user_id, session_token))
    }

    async fn make_unique_username(&self, base: &str) -> String {
        let mut name = base.to_string();
        let mut counter = 1u32;
        loop {
            let exists: Option<(bool,)> = sqlx::query_as(
                "SELECT EXISTS(SELECT 1 FROM users WHERE LOWER(username) = LOWER($1))",
            )
            .bind(&name)
            .fetch_one(&self.pool)
            .await
            .ok();
            if !exists.map(|(value,)| value).unwrap_or(true) {
                return name;
            }
            counter += 1;
            if counter > 100 {
                let suffix: String = thread_rng()
                    .sample_iter(&Alphanumeric)
                    .take(4)
                    .map(char::from)
                    .collect();
                return format!("{}_{}", base, suffix);
            }
            name = format!("{}_{}", base, counter);
        }
    }
}
