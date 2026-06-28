//! 小游戏排行榜 REST 接口（OpenSpec: minigame-leaderboard-service）。
//!
//! POST /api/minigame/{game_key}/score          (SessionToken) 上报并 upsert 每关最佳
//! GET  /api/minigame/{game_key}/leaderboard     公开，分页 Top 榜
//! GET  /api/minigame/{game_key}/leaderboard/me  (SessionToken) 我的名次

use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::json;

use crate::api::get_session_token;
use crate::database::DatabaseManager;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/{game_key}/score", web::post().to(submit_score))
        .route("/{game_key}/leaderboard", web::get().to(get_leaderboard))
        .route("/{game_key}/leaderboard/me", web::get().to(get_my_rank));
}

const PAGE_SIZE: i64 = 50;

/// game_key 校验，沿用 minigame-config 的约定。
fn validate_game_key(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 64
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[derive(Deserialize)]
struct ScoreReq {
    level_id: i32,
    score: i32,
    stars: i32,
}

#[derive(Deserialize)]
struct PageQuery {
    page: Option<i64>,
}

/// 从 SessionToken 头解析出 user_id 字符串。
async fn resolve_user_id(req: &HttpRequest, db: &DatabaseManager) -> Option<String> {
    let token = get_session_token(req)?;
    let user = db.users.get_session_token(token, &db.friendships).await?;
    Some(user.id.to_string())
}

async fn submit_score(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<String>,
    body: web::Json<ScoreReq>,
) -> HttpResponse {
    let game_key = path.into_inner();
    if !validate_game_key(&game_key) {
        return HttpResponse::BadRequest().body("invalid game_key");
    }
    let Some(user_id) = resolve_user_id(&req, &db).await else {
        return HttpResponse::Unauthorized().finish();
    };
    if body.level_id <= 0 || body.score < 0 {
        return HttpResponse::BadRequest().body("invalid score payload");
    }
    let stars = body.stars.clamp(0, 3) as i16;

    match db
        .minigame_leaderboard
        .submit_score(&user_id, &game_key, body.level_id, body.score, stars)
        .await
    {
        Ok((best_score, best_stars)) => {
            let me = db
                .minigame_leaderboard
                .get_user_rank(&game_key, &user_id)
                .await;
            let (total_score, total_stars, rank) = me
                .map(|m| (m.total_score, m.total_stars, m.rank))
                .unwrap_or((best_score as i64, best_stars as i64, 0));
            HttpResponse::Ok().json(json!({
                "level_id": body.level_id,
                "best_score": best_score,
                "best_stars": best_stars,
                "total_score": total_score,
                "total_stars": total_stars,
                "rank": rank,
            }))
        }
        Err(e) => {
            log::error!("minigame submit_score error: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

async fn get_leaderboard(
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<String>,
    q: web::Query<PageQuery>,
) -> HttpResponse {
    let game_key = path.into_inner();
    if !validate_game_key(&game_key) {
        return HttpResponse::BadRequest().body("invalid game_key");
    }
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page - 1) * PAGE_SIZE;
    let entries = db
        .minigame_leaderboard
        .get_top(&game_key, PAGE_SIZE, offset)
        .await;
    HttpResponse::Ok().json(entries)
}

async fn get_my_rank(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<String>,
) -> HttpResponse {
    let game_key = path.into_inner();
    if !validate_game_key(&game_key) {
        return HttpResponse::BadRequest().body("invalid game_key");
    }
    let Some(user_id) = resolve_user_id(&req, &db).await else {
        return HttpResponse::Unauthorized().finish();
    };
    match db
        .minigame_leaderboard
        .get_user_rank(&game_key, &user_id)
        .await
    {
        Some(me) => HttpResponse::Ok().json(me),
        None => HttpResponse::NotFound().finish(),
    }
}
