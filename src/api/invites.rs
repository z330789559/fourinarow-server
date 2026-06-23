use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::{
    api::{get_session_token, ApiResponse},
    database::DatabaseManager,
    player::RedeemError,
};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("", web::get().to(list_invites))
        .route("", web::post().to(create_invite))
        .route("/redeem", web::post().to(redeem_invite));
}

#[derive(Deserialize)]
struct CreateInviteReq {
    max_uses: Option<i32>,
}

#[derive(Deserialize)]
struct RedeemInviteReq {
    code: String,
}

#[derive(Serialize)]
struct RedeemResp {
    message: String,
    reward_item_id: Option<String>,
    reward_quantity: Option<i32>,
}

async fn list_invites(req: HttpRequest, db: web::Data<Arc<DatabaseManager>>) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    HttpResponse::Ok().json(db.invites.list_by_creator(&user.id).await)
}

async fn create_invite(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    payload: web::Json<CreateInviteReq>,
) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    let max_uses = payload.max_uses.unwrap_or(1).clamp(1, 100);
    match db.invites.create(&user.id, max_uses).await {
        Some(invite) => HttpResponse::Ok().json(invite),
        None => {
            HttpResponse::InternalServerError().json(ApiResponse::new("Failed to create invite"))
        }
    }
}

async fn redeem_invite(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    payload: web::Json<RedeemInviteReq>,
) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    match db.players.redeem_invite(&payload.code, &user.id).await {
        Ok(maybe_reward) => HttpResponse::Ok().json(RedeemResp {
            message: "Invite redeemed successfully".to_string(),
            reward_item_id: maybe_reward.as_ref().map(|(item_id, _)| item_id.clone()),
            reward_quantity: maybe_reward.map(|(_, qty)| qty),
        }),
        Err(RedeemError::InvalidCode) => {
            HttpResponse::BadRequest().json(ApiResponse::new("Invalid invite code"))
        }
        Err(RedeemError::Expired) => {
            HttpResponse::BadRequest().json(ApiResponse::new("Invite code has expired"))
        }
        Err(RedeemError::MaxUsesReached) => HttpResponse::BadRequest()
            .json(ApiResponse::new("Invite code has reached its maximum uses")),
        Err(RedeemError::AlreadyUsed) => HttpResponse::BadRequest()
            .json(ApiResponse::new("You have already used this invite code")),
        Err(RedeemError::CacheFlush(error)) => {
            log::error!(
                "failed to flush player cache before invite redeem: {:?}",
                error
            );
            HttpResponse::InternalServerError().json(ApiResponse::new("Failed to redeem invite"))
        }
        Err(RedeemError::Db(error)) => {
            log::error!("failed to redeem invite: {:?}", error);
            HttpResponse::InternalServerError().json(ApiResponse::new("Failed to redeem invite"))
        }
    }
}
