use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};

use crate::{
    api::get_session_token,
    database::{
        inbox::{claim_inbox, delete_inbox, list_inbox, mark_read, ClaimError},
        DatabaseManager,
    },
};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("", web::get().to(get_inbox))
        .route("/{id}/claim", web::post().to(claim_handler))
        .route("/{id}/read", web::post().to(read_handler))
        .route("/{id}", web::delete().to(delete_handler));
}

async fn get_inbox(req: HttpRequest, db: web::Data<Arc<DatabaseManager>>) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };
    HttpResponse::Ok().json(list_inbox(&db.pool, &user.id).await)
}

async fn claim_handler(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    id: web::Path<i64>,
) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    match claim_inbox(&db.pool, &user.id, id.into_inner()).await {
        Ok((item_id, qty)) => HttpResponse::Ok().json(serde_json::json!({
            "reward_item_id": item_id,
            "reward_qty": qty,
        })),
        Err(ClaimError::NotFound) => HttpResponse::NotFound().body("Message not found"),
        Err(ClaimError::AlreadyClaimed) => HttpResponse::Conflict().body("Already claimed"),
        Err(ClaimError::NoReward) => HttpResponse::BadRequest().body("No reward to claim"),
        Err(ClaimError::Db(e)) => {
            log::error!("inbox claim db error: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

async fn read_handler(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    id: web::Path<i64>,
) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    mark_read(&db.pool, &user.id, id.into_inner()).await;
    HttpResponse::Ok().json(serde_json::json!({}))
}

async fn delete_handler(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    id: web::Path<i64>,
) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    match delete_inbox(&db.pool, &user.id, id.into_inner()).await {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({})),
        Err("not_found") => HttpResponse::NotFound().body("Message not found"),
        Err("unclaimed_reward") => HttpResponse::BadRequest().body("Please claim reward first"),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}
