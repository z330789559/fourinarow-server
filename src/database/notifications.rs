use std::collections::HashMap;

use sqlx::PgPool;

use crate::api::users::user::UserId;

pub const MODULE_QUESTS: &str = "quests";
pub const MODULE_ACHIEVEMENTS: &str = "achievements";
pub const MODULE_FRIENDS: &str = "friends";
pub const MODULE_INBOX: &str = "inbox";

pub async fn set_badge(pool: &PgPool, user_id: &UserId, module: &str) {
    let _ = sqlx::query(
        "INSERT INTO user_notification_badges (user_id, module, has_new, updated_at) \
         VALUES ($1, $2, true, NOW()) \
         ON CONFLICT (user_id, module) DO UPDATE SET has_new = true, updated_at = NOW()",
    )
    .bind(user_id.to_string())
    .bind(module)
    .execute(pool)
    .await;
}

pub async fn clear_badge(pool: &PgPool, user_id: &UserId, module: &str) {
    let _ = sqlx::query(
        "UPDATE user_notification_badges SET has_new = false, updated_at = NOW() \
         WHERE user_id = $1 AND module = $2",
    )
    .bind(user_id.to_string())
    .bind(module)
    .execute(pool)
    .await;
}

pub async fn get_badges(pool: &PgPool, user_id: &UserId) -> HashMap<String, bool> {
    let modules = [MODULE_QUESTS, MODULE_ACHIEVEMENTS, MODULE_FRIENDS, MODULE_INBOX];
    let rows: Vec<(String, bool)> = sqlx::query_as(
        "SELECT module, has_new FROM user_notification_badges WHERE user_id = $1",
    )
    .bind(user_id.to_string())
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let db_map: HashMap<String, bool> = rows.into_iter().collect();
    modules
        .iter()
        .map(|m| (m.to_string(), *db_map.get(*m).unwrap_or(&false)))
        .collect()
}
