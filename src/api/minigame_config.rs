// 小游戏配置版本化下发：REST 接口层（OpenSpec: minigame-config-version-service）。
// 读接口公开（manifest / active(ETag→304) / versions/{version}）；写接口需 admin token（fail closed）。

use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::database::{
    minigame_config::{
        create_draft, get_active, get_manifest, get_version, list_versions, publish, rollback,
        WriteError,
    },
    DatabaseManager,
};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/{game_key}/manifest", web::get().to(get_manifest_h))
        .route("/{game_key}/active", web::get().to(get_active_h))
        .route("/{game_key}/versions", web::get().to(list_versions_h))
        .route("/{game_key}/versions", web::post().to(create_version_h))
        .route("/{game_key}/versions/{version}", web::get().to(get_version_h))
        .route(
            "/{game_key}/versions/{version}/publish",
            web::post().to(publish_h),
        )
        .route("/{game_key}/rollback", web::post().to(rollback_h));
}

// ---- 查询参数 ----

#[derive(Deserialize)]
struct ChannelQuery {
    channel: Option<String>,
}

#[derive(Deserialize)]
struct RollbackQuery {
    channel: Option<String>,
    to: i32,
}

#[derive(Deserialize)]
struct CreateReq {
    channel: Option<String>,
    level: Value,
    scene: Value,
    mode: Value,
    note: Option<String>,
}

// ---- 校验 / 鉴权 ----

fn resolve_channel(raw: &Option<String>) -> Result<String, HttpResponse> {
    let c = raw.clone().unwrap_or_else(|| "production".to_string());
    if c == "production" || c == "staging" {
        Ok(c)
    } else {
        Err(HttpResponse::BadRequest().body("invalid channel (production|staging)"))
    }
}

fn validate_game_key(k: &str) -> Result<(), HttpResponse> {
    let ok = !k.is_empty()
        && k.len() <= 64
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(HttpResponse::BadRequest().body("invalid game_key"))
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// 管理鉴权：fail closed —— 未配置 token 一律拒绝；header `x-admin-token` 常量时间比对。
fn require_admin(req: &HttpRequest) -> Result<(), HttpResponse> {
    let token = std::env::var("MINIGAME_CONFIG_ADMIN_TOKEN").unwrap_or_default();
    if token.is_empty() {
        return Err(HttpResponse::ServiceUnavailable().body("admin token not configured"));
    }
    let provided = req
        .headers()
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if constant_time_eq(provided.as_bytes(), token.as_bytes()) {
        Ok(())
    } else {
        Err(HttpResponse::Unauthorized().body("invalid admin token"))
    }
}

// ---- checksum ----

fn canonical(v: &Value) -> String {
    // serde_json::Value 默认 BTreeMap（键有序），紧凑、保留 UTF-8，与种子 python canonical 一致
    serde_json::to_string(v).unwrap_or_default()
}

fn compute_checksum(level: &Value, scene: &Value, mode: &Value) -> String {
    let blob = format!(
        "{}\n{}\n{}",
        canonical(level),
        canonical(scene),
        canonical(mode)
    );
    let mut hasher = Sha256::new();
    hasher.update(blob.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn write_error_response(e: WriteError) -> HttpResponse {
    match e {
        WriteError::NotFound => HttpResponse::NotFound().body("version not found"),
        WriteError::NotPublished => {
            HttpResponse::BadRequest().body("target version is not published")
        }
        WriteError::Db(err) => {
            log::error!("minigame_config db error: {:?}", err);
            HttpResponse::InternalServerError().finish()
        }
    }
}

// ---- 只读（公开）----

async fn get_manifest_h(
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<String>,
    q: web::Query<ChannelQuery>,
) -> HttpResponse {
    let game_key = path.into_inner();
    if let Err(r) = validate_game_key(&game_key) {
        return r;
    }
    let channel = match resolve_channel(&q.channel) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match get_manifest(&db.pool, &game_key, &channel).await {
        Some(m) => HttpResponse::Ok().json(m),
        None => HttpResponse::NotFound().body("no active version"),
    }
}

async fn get_active_h(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<String>,
    q: web::Query<ChannelQuery>,
) -> HttpResponse {
    let game_key = path.into_inner();
    if let Err(r) = validate_game_key(&game_key) {
        return r;
    }
    let channel = match resolve_channel(&q.channel) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Some(bundle) = get_active(&db.pool, &game_key, &channel).await else {
        return HttpResponse::NotFound().body("no active version");
    };
    let etag = format!("\"{}\"", bundle.checksum);

    // If-None-Match 命中 checksum → 304
    if let Some(inm) = req
        .headers()
        .get("If-None-Match")
        .and_then(|v| v.to_str().ok())
    {
        let hit = inm.split(',').any(|t| {
            t.trim().trim_start_matches("W/").trim_matches('"') == bundle.checksum
        });
        if hit {
            return HttpResponse::NotModified().insert_header(("ETag", etag)).finish();
        }
    }

    let body = json!({
        "gameKey": bundle.game_key,
        "channel": bundle.channel,
        "version": bundle.version,
        "checksum": bundle.checksum,
        "level": bundle.level,
        "scene": bundle.scene,
        "mode": bundle.mode,
    });
    HttpResponse::Ok()
        .insert_header(("ETag", etag))
        .insert_header(("Cache-Control", "no-cache"))
        .json(body)
}

async fn get_version_h(
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<(String, i32)>,
    q: web::Query<ChannelQuery>,
) -> HttpResponse {
    let (game_key, version) = path.into_inner();
    if let Err(r) = validate_game_key(&game_key) {
        return r;
    }
    let channel = match resolve_channel(&q.channel) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match get_version(&db.pool, &game_key, &channel, version).await {
        Some(bundle) => HttpResponse::Ok().json(json!({
            "gameKey": bundle.game_key,
            "channel": bundle.channel,
            "version": bundle.version,
            "checksum": bundle.checksum,
            "level": bundle.level,
            "scene": bundle.scene,
            "mode": bundle.mode,
        })),
        None => HttpResponse::NotFound().body("version not found"),
    }
}

// ---- 管理（需 admin token）----

async fn list_versions_h(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<String>,
    q: web::Query<ChannelQuery>,
) -> HttpResponse {
    if let Err(r) = require_admin(&req) {
        return r;
    }
    let game_key = path.into_inner();
    if let Err(r) = validate_game_key(&game_key) {
        return r;
    }
    let channel = match resolve_channel(&q.channel) {
        Ok(c) => c,
        Err(r) => return r,
    };
    HttpResponse::Ok().json(list_versions(&db.pool, &game_key, &channel).await)
}

async fn create_version_h(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<String>,
    body: web::Json<CreateReq>,
) -> HttpResponse {
    if let Err(r) = require_admin(&req) {
        return r;
    }
    let game_key = path.into_inner();
    if let Err(r) = validate_game_key(&game_key) {
        return r;
    }
    let channel = match resolve_channel(&body.channel) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if !body.level.is_array() || !body.scene.is_array() || !body.mode.is_array() {
        return HttpResponse::BadRequest().body("level/scene/mode must each be a JSON array");
    }

    let checksum = compute_checksum(&body.level, &body.scene, &body.mode);
    let level = canonical(&body.level);
    let scene = canonical(&body.scene);
    let mode = canonical(&body.mode);
    let note = body.note.as_deref();

    match create_draft(
        &db.pool, &game_key, &channel, &level, &scene, &mode, &checksum, note, Some("admin"),
    )
    .await
    {
        Ok(version) => HttpResponse::Ok().json(json!({
            "version": version,
            "checksum": checksum,
            "status": "draft",
        })),
        Err(e) => {
            log::error!("minigame_config create_draft error: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

async fn publish_h(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<(String, i32)>,
    q: web::Query<ChannelQuery>,
) -> HttpResponse {
    if let Err(r) = require_admin(&req) {
        return r;
    }
    let (game_key, version) = path.into_inner();
    if let Err(r) = validate_game_key(&game_key) {
        return r;
    }
    let channel = match resolve_channel(&q.channel) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match publish(&db.pool, &game_key, &channel, version).await {
        Ok(()) => HttpResponse::Ok().json(json!({
            "gameKey": game_key, "channel": channel, "activeVersion": version,
        })),
        Err(e) => write_error_response(e),
    }
}

async fn rollback_h(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<String>,
    q: web::Query<RollbackQuery>,
) -> HttpResponse {
    if let Err(r) = require_admin(&req) {
        return r;
    }
    let game_key = path.into_inner();
    if let Err(r) = validate_game_key(&game_key) {
        return r;
    }
    let channel = match resolve_channel(&q.channel) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match rollback(&db.pool, &game_key, &channel, q.to).await {
        Ok(()) => HttpResponse::Ok().json(json!({
            "gameKey": game_key, "channel": channel, "activeVersion": q.to,
        })),
        Err(e) => write_error_response(e),
    }
}
