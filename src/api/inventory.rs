use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};

use crate::{api::get_session_token, database::DatabaseManager};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("", web::get().to(get_inventory))
        .route("/shop/{shop_id}", web::get().to(get_shop))
        .route("/shop/{shop_id}/buy/{item_id}", web::post().to(buy_item));
}

async fn get_inventory(req: HttpRequest, db: web::Data<Arc<DatabaseManager>>) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    HttpResponse::Ok().json(db.items.get_inventory(&user.id).await)
}

async fn get_shop(db: web::Data<Arc<DatabaseManager>>, shop_id: web::Path<String>) -> HttpResponse {
    HttpResponse::Ok().json(db.items.get_shop(&shop_id).await)
}

async fn buy_item(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<(String, String)>,
) -> HttpResponse {
    let (shop_id, item_id) = path.into_inner();
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    match db.items.purchase(&user.id, &shop_id, &item_id).await {
        Ok(()) => HttpResponse::Ok().finish(),
        Err(crate::database::items::ItemError::NotEnoughItems) => {
            HttpResponse::BadRequest().body("Not enough items")
        }
        Err(crate::database::items::ItemError::ItemNotFound) => {
            HttpResponse::NotFound().body("Item not found in shop")
        }
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}
