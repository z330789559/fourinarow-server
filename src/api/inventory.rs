use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};

use crate::{
    api::get_session_token,
    database::{items::InventoryEntry, notifications::{set_badge, MODULE_QUESTS}, DatabaseManager},
    logging::{ActivityEvent, ActivityEventKind, ActivityLogHandle},
};

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

    match db.players.get_readonly(&user.id).await {
        Ok(player) => {
            let inventory = player
                .inventory
                .into_iter()
                .map(|(item_id, quantity)| InventoryEntry { item_id, quantity })
                .collect::<Vec<_>>();
            HttpResponse::Ok().json(inventory)
        }
        Err(error) => {
            log::error!("failed to load player inventory {}: {:?}", user.id, error);
            HttpResponse::InternalServerError().finish()
        }
    }
}

async fn get_shop(db: web::Data<Arc<DatabaseManager>>, shop_id: web::Path<String>) -> HttpResponse {
    HttpResponse::Ok().json(db.items.get_shop(&shop_id).await)
}

async fn buy_item(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    activity_log: web::Data<ActivityLogHandle>,
    path: web::Path<(String, String)>,
) -> HttpResponse {
    let (shop_id, item_id) = path.into_inner();
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    let request_id = req
        .headers()
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if request_id.is_some_and(|value| value.len() > 96) {
        return HttpResponse::BadRequest().body("Idempotency-Key is too long");
    }

    match db
        .players
        .purchase(&user.id, &shop_id, &item_id, request_id)
        .await
    {
        Ok(completed_quests) => {
            activity_log.record(ActivityEvent::new(
                Some(user.id.to_string()),
                ActivityEventKind::Purchase,
                Some(serde_json::json!({ "shop_id": shop_id, "item_id": item_id })),
            ));
            if !completed_quests.is_empty() {
                set_badge(&db.pool, &user.id, MODULE_QUESTS).await;
            }
            HttpResponse::Ok().finish()
        }
        Err(crate::player::PurchaseError::NotEnoughItems) => {
            HttpResponse::BadRequest().body("Not enough items")
        }
        Err(crate::player::PurchaseError::ItemNotFound) => {
            HttpResponse::NotFound().body("Item not found in shop")
        }
        Err(crate::player::PurchaseError::AlreadyApplied) => HttpResponse::Conflict().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}
