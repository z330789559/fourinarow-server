use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};

use crate::{api::get_session_token, database::DatabaseManager};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/story", web::get().to(story_progress))
        .route("/daily", web::get().to(daily_progress))
        .route("/achievements", web::get().to(achievement_progress));
}

async fn story_progress(req: HttpRequest, db: web::Data<Arc<DatabaseManager>>) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    HttpResponse::Ok().json(db.quests.get_story_progress(&user.id).await)
}

async fn daily_progress(req: HttpRequest, db: web::Data<Arc<DatabaseManager>>) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    HttpResponse::Ok().json(db.quests.get_daily_progress(&user.id).await)
}

async fn achievement_progress(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    let progress = db.quests.get_achievement_progress(&user.id).await;
    let response: Vec<serde_json::Value> = progress
        .into_iter()
        .map(|(id, tier, value)| {
            serde_json::json!({
                "achievement_id": id,
                "current_tier": tier,
                "current_value": value,
            })
        })
        .collect();

    HttpResponse::Ok().json(response)
}
