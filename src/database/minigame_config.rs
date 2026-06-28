// 小游戏配置版本化下发：数据访问层（OpenSpec: minigame-config-version-service）。
// 沿用项目 runtime sqlx 查询风格（非 query! 宏，cargo check 不依赖在线 DB）。
// JSONB 列读出用 `::text`→String、写入用 `$n::jsonb` 绑定 String，避免开启 sqlx 的 json feature。

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

/// 现役版本清单（仅元信息，供客户端廉价版本协商）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigManifest {
    pub game_key: String,
    pub channel: String,
    pub version: i32,
    pub checksum: String,
    pub published_at: Option<DateTime<Utc>>,
}

/// 配置整包（level/scene/mode 三份 JSON）。
pub struct ConfigBundle {
    pub game_key: String,
    pub channel: String,
    pub version: i32,
    pub checksum: String,
    pub level: Value,
    pub scene: Value,
    pub mode: Value,
}

/// 版本历史元信息（不含整包）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigVersionMeta {
    pub version: i32,
    pub status: String,
    pub checksum: String,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

/// publish / rollback 失败原因。
#[derive(Debug)]
pub enum WriteError {
    NotFound,
    NotPublished,
    Db(sqlx::Error),
}
impl From<sqlx::Error> for WriteError {
    fn from(e: sqlx::Error) -> Self {
        WriteError::Db(e)
    }
}

fn parse_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or(Value::Null)
}

/// 现役版本清单（经 active 指针解析）。
pub async fn get_manifest(pool: &PgPool, game_key: &str, channel: &str) -> Option<ConfigManifest> {
    let row: Option<(i32, String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT v.version, v.checksum, v.published_at \
         FROM minigame_config_active a \
         JOIN minigame_config_version v ON v.id = a.version_id \
         WHERE a.game_key = $1 AND a.channel = $2",
    )
    .bind(game_key)
    .bind(channel)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    row.map(|(version, checksum, published_at)| ConfigManifest {
        game_key: game_key.to_string(),
        channel: channel.to_string(),
        version,
        checksum,
        published_at,
    })
}

/// 现役整包。
pub async fn get_active(pool: &PgPool, game_key: &str, channel: &str) -> Option<ConfigBundle> {
    let row: Option<(i32, String, String, String, String)> = sqlx::query_as(
        "SELECT v.version, v.checksum, v.level_config::text, v.scene_config::text, v.mode_config::text \
         FROM minigame_config_active a \
         JOIN minigame_config_version v ON v.id = a.version_id \
         WHERE a.game_key = $1 AND a.channel = $2",
    )
    .bind(game_key)
    .bind(channel)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    row.map(|(version, checksum, level, scene, mode)| ConfigBundle {
        game_key: game_key.to_string(),
        channel: channel.to_string(),
        version,
        checksum,
        level: parse_json(&level),
        scene: parse_json(&scene),
        mode: parse_json(&mode),
    })
}

/// 指定版本整包。
pub async fn get_version(
    pool: &PgPool,
    game_key: &str,
    channel: &str,
    version: i32,
) -> Option<ConfigBundle> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT v.checksum, v.level_config::text, v.scene_config::text, v.mode_config::text \
         FROM minigame_config_version v \
         WHERE v.game_key = $1 AND v.channel = $2 AND v.version = $3",
    )
    .bind(game_key)
    .bind(channel)
    .bind(version)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    row.map(|(checksum, level, scene, mode)| ConfigBundle {
        game_key: game_key.to_string(),
        channel: channel.to_string(),
        version,
        checksum,
        level: parse_json(&level),
        scene: parse_json(&scene),
        mode: parse_json(&mode),
    })
}

/// 版本历史列表（不含整包）。
pub async fn list_versions(
    pool: &PgPool,
    game_key: &str,
    channel: &str,
) -> Vec<ConfigVersionMeta> {
    let rows: Vec<(i32, String, String, Option<String>, DateTime<Utc>, Option<DateTime<Utc>>)> =
        sqlx::query_as(
            "SELECT version, status, checksum, note, created_at, published_at \
             FROM minigame_config_version \
             WHERE game_key = $1 AND channel = $2 \
             ORDER BY version DESC",
        )
        .bind(game_key)
        .bind(channel)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    rows.into_iter()
        .map(
            |(version, status, checksum, note, created_at, published_at)| ConfigVersionMeta {
                version,
                status,
                checksum,
                note,
                created_at,
                published_at,
            },
        )
        .collect()
}

/// 新建 draft 版本：版本号 = (game_key, channel) 内 max+1（子查询计算）；level/scene/mode 为 canonical JSON 字符串。
/// 不改变现役指针。返回新版本号。
pub async fn create_draft(
    pool: &PgPool,
    game_key: &str,
    channel: &str,
    level_json: &str,
    scene_json: &str,
    mode_json: &str,
    checksum: &str,
    note: Option<&str>,
    created_by: Option<&str>,
) -> Result<i32, sqlx::Error> {
    let row: (i32,) = sqlx::query_as(
        "INSERT INTO minigame_config_version \
            (game_key, channel, version, status, level_config, scene_config, mode_config, checksum, note, created_by) \
         VALUES ($1, $2, \
            (SELECT COALESCE(MAX(version), 0) + 1 FROM minigame_config_version WHERE game_key = $1 AND channel = $2), \
            'draft', $3::jsonb, $4::jsonb, $5::jsonb, $6, $7, $8) \
         RETURNING version",
    )
    .bind(game_key)
    .bind(channel)
    .bind(level_json)
    .bind(scene_json)
    .bind(mode_json)
    .bind(checksum)
    .bind(note)
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// 发布版本：事务内置 published + 翻转 active 指针。
pub async fn publish(
    pool: &PgPool,
    game_key: &str,
    channel: &str,
    version: i32,
) -> Result<(), WriteError> {
    let mut tx = pool.begin().await?;

    let updated = sqlx::query(
        "UPDATE minigame_config_version \
         SET status = 'published', published_at = COALESCE(published_at, now()) \
         WHERE game_key = $1 AND channel = $2 AND version = $3",
    )
    .bind(game_key)
    .bind(channel)
    .bind(version)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(WriteError::NotFound);
    }

    sqlx::query(
        "INSERT INTO minigame_config_active (game_key, channel, version_id) \
         SELECT game_key, channel, id FROM minigame_config_version \
         WHERE game_key = $1 AND channel = $2 AND version = $3 \
         ON CONFLICT (game_key, channel) DO UPDATE SET version_id = EXCLUDED.version_id, updated_at = now()",
    )
    .bind(game_key)
    .bind(channel)
    .bind(version)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// 回滚：active 指针指向一个已 published 的历史版本。
pub async fn rollback(
    pool: &PgPool,
    game_key: &str,
    channel: &str,
    to_version: i32,
) -> Result<(), WriteError> {
    let mut tx = pool.begin().await?;

    let target: Option<(String,)> = sqlx::query_as(
        "SELECT status FROM minigame_config_version \
         WHERE game_key = $1 AND channel = $2 AND version = $3",
    )
    .bind(game_key)
    .bind(channel)
    .bind(to_version)
    .fetch_optional(&mut *tx)
    .await?;

    match target {
        None => return Err(WriteError::NotFound),
        Some((status,)) if status != "published" => return Err(WriteError::NotPublished),
        Some(_) => {}
    }

    sqlx::query(
        "INSERT INTO minigame_config_active (game_key, channel, version_id) \
         SELECT game_key, channel, id FROM minigame_config_version \
         WHERE game_key = $1 AND channel = $2 AND version = $3 \
         ON CONFLICT (game_key, channel) DO UPDATE SET version_id = EXCLUDED.version_id, updated_at = now()",
    )
    .bind(game_key)
    .bind(channel)
    .bind(to_version)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}
