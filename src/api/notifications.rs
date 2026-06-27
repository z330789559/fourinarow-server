use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};

use crate::{
    api::get_session_token,
    database::{
        notifications::{clear_badge, get_badges, MODULE_ACHIEVEMENTS, MODULE_FRIENDS, MODULE_INBOX, MODULE_QUESTS},
        DatabaseManager,
    },
};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/badges", web::get().to(get_badges_handler))
        .route("/badges/{module}/clear", web::post().to(clear_badge_handler));
}

async fn get_badges_handler(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    let badges = get_badges(&db.pool, &user.id).await;
    HttpResponse::Ok().json(badges)
}

async fn clear_badge_handler(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    module: web::Path<String>,
) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    let module = module.into_inner();
    let valid = [MODULE_QUESTS, MODULE_ACHIEVEMENTS, MODULE_FRIENDS, MODULE_INBOX];
    if !valid.contains(&module.as_str()) {
        return HttpResponse::BadRequest().body("Invalid module");
    }

    clear_badge(&db.pool, &user.id, &module).await;
    HttpResponse::Ok().json(serde_json::json!({}))
}
