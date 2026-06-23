use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;

use crate::database::DatabaseManager;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("", web::get().to(get_leaderboard))
        .route("/me", web::get().to(get_my_rank));
}

#[derive(Deserialize)]
struct LeaderboardQuery {
    #[serde(rename = "type")]
    board_type: Option<String>,
    page: Option<i64>,
}

async fn get_leaderboard(
    db: web::Data<Arc<DatabaseManager>>,
    query: web::Query<LeaderboardQuery>,
) -> HttpResponse {
    let page = query.page.unwrap_or(1).max(1);
    let limit = 50_i64;
    let offset = (page - 1) * limit;
    let board_type = query.board_type.as_deref().unwrap_or("skill_rating");

    let entries = match board_type {
        "wins" => db.leaderboard.get_top_by_wins(limit, offset).await,
        _ => db.leaderboard.get_top_by_rating(limit, offset).await,
    };

    HttpResponse::Ok().json(entries)
}

async fn get_my_rank(req: HttpRequest, db: web::Data<Arc<DatabaseManager>>) -> HttpResponse {
    use crate::api::get_session_token;

    let Some(session_token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db
        .users
        .get_session_token(session_token, &db.friendships)
        .await
    else {
        return HttpResponse::Unauthorized().finish();
    };

    match db.leaderboard.get_user_rank(&user.id.to_string()).await {
        Some(entry) => HttpResponse::Ok().json(entry),
        None => HttpResponse::NotFound().finish(),
    }
}
