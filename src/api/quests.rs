use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use chrono::Utc;
use serde::Serialize;
use serde_json;

use crate::{
    api::get_session_token,
    database::DatabaseManager,
    logging::{ActivityEvent, ActivityEventKind, ActivityLogHandle},
    player::{QuestClaimError, QuestClaimKind, QuestClaimReward},
};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/story", web::get().to(story_progress))
        .route("/daily", web::get().to(daily_progress))
        .route("/achievements", web::get().to(achievement_progress))
        .route(
            "/story/{quest_id}/claim",
            web::post().to(claim_story_reward),
        )
        .route(
            "/daily/{quest_id}/claim",
            web::post().to(claim_daily_reward),
        );
}

#[derive(Serialize)]
struct QuestClaimResp {
    quest_id: String,
    reward_item_id: Option<String>,
    reward_quantity: i32,
}

impl From<QuestClaimReward> for QuestClaimResp {
    fn from(reward: QuestClaimReward) -> Self {
        QuestClaimResp {
            quest_id: reward.quest_id,
            reward_item_id: reward.reward_item_id,
            reward_quantity: reward.reward_quantity,
        }
    }
}

async fn story_progress(req: HttpRequest, db: web::Data<Arc<DatabaseManager>>) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    match db.players.story_quest_progress(&user.id).await {
        Ok(progress) => HttpResponse::Ok().json(progress),
        Err(error) => {
            log::error!("failed to load story quest progress {}: {:?}", user.id, error);
            HttpResponse::InternalServerError().finish()
        }
    }
}

async fn daily_progress(req: HttpRequest, db: web::Data<Arc<DatabaseManager>>) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    let today = Utc::now().date_naive();
    match db.players.daily_quest_progress(&user.id, today).await {
        Ok(progress) => HttpResponse::Ok().json(progress),
        Err(error) => {
            log::error!("failed to load daily quest progress {}: {:?}", user.id, error);
            HttpResponse::InternalServerError().finish()
        }
    }
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

    let player = match db.players.get_readonly(&user.id).await {
        Ok(p) => p,
        Err(error) => {
            log::error!("failed to load achievement progress {}: {:?}", user.id, error);
            return HttpResponse::InternalServerError().finish();
        }
    };
    let progressive = match db.players.get_progressive_achievement_progress(&user.id).await {
        Ok(p) => p,
        Err(error) => {
            log::error!("failed to load progressive achievements {}: {:?}", user.id, error);
            return HttpResponse::InternalServerError().finish();
        }
    };

    let mut response: Vec<serde_json::Value> = player
        .achievements
        .into_iter()
        .map(|a| {
            serde_json::json!({
                "kind": "tiered",
                "achievement_id": a.achievement_id,
                "current_tier": a.current_tier,
                "current_value": a.current_value,
            })
        })
        .collect();

    for p in progressive {
        let reward_preview: Vec<serde_json::Value> = p
            .reward_preview
            .into_iter()
            .map(|(item_id, amount)| serde_json::json!({"item_id": item_id, "amount": amount}))
            .collect();
        response.push(serde_json::json!({
            "kind": "progressive",
            "achievement_id": p.achievement_id,
            "name": p.name,
            "mode": p.mode,
            "step": p.step,
            "current_target_level": p.current_target_level,
            "reward_preview": reward_preview,
        }));
    }

    HttpResponse::Ok().json(response)
}

async fn claim_story_reward(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    activity_log: web::Data<ActivityLogHandle>,
    quest_id: web::Path<String>,
) -> HttpResponse {
    claim_reward(req, db, activity_log, QuestClaimKind::Story, quest_id.into_inner()).await
}

async fn claim_daily_reward(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    activity_log: web::Data<ActivityLogHandle>,
    quest_id: web::Path<String>,
) -> HttpResponse {
    claim_reward(req, db, activity_log, QuestClaimKind::Daily, quest_id.into_inner()).await
}

async fn claim_reward(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    activity_log: web::Data<ActivityLogHandle>,
    kind: QuestClaimKind,
    quest_id: String,
) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    match db
        .players
        .claim_quest_reward(&user.id, kind, &quest_id)
        .await
    {
        Ok(reward) => {
            activity_log.record(ActivityEvent::new(
                Some(user.id.to_string()),
                ActivityEventKind::ClaimReward,
                Some(serde_json::json!({ "quest_id": quest_id })),
            ));
            HttpResponse::Ok().json(QuestClaimResp::from(reward))
        }
        Err(QuestClaimError::NotFound) => HttpResponse::NotFound().body("Quest reward not found"),
        Err(QuestClaimError::NotCompleted) => {
            HttpResponse::BadRequest().body("Quest is not completed")
        }
        Err(QuestClaimError::AlreadyClaimed) => {
            HttpResponse::Conflict().body("Quest reward already claimed")
        }
        Err(error) => {
            log::error!(
                "failed to claim quest reward user_id={} kind={:?} quest_id={}: {:?}",
                user.id,
                kind,
                quest_id,
                error
            );
            HttpResponse::InternalServerError().finish()
        }
    }
}
