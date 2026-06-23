use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use chrono::Utc;
use serde::Serialize;

use crate::{
    api::get_session_token,
    database::{quests::QuestProgress, DatabaseManager},
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

    match db.players.get_readonly(&user.id).await {
        Ok(player) => {
            let progress = player
                .quests
                .into_iter()
                .filter(|quest| quest.quest_date.is_none())
                .map(|quest| QuestProgress {
                    quest_id: quest.quest_id,
                    current_value: quest.current_value,
                    completed_at: quest.completed_at,
                    rewarded: quest.rewarded,
                    quest_date: quest.quest_date,
                })
                .collect::<Vec<_>>();
            HttpResponse::Ok().json(progress)
        }
        Err(error) => {
            log::error!(
                "failed to load story quest progress {}: {:?}",
                user.id,
                error
            );
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

    match db.players.get_readonly(&user.id).await {
        Ok(player) => {
            let today = Utc::now().date_naive();
            let progress = player
                .quests
                .into_iter()
                .filter(|quest| quest.quest_date == Some(today))
                .map(|quest| QuestProgress {
                    quest_id: quest.quest_id,
                    current_value: quest.current_value,
                    completed_at: quest.completed_at,
                    rewarded: quest.rewarded,
                    quest_date: quest.quest_date,
                })
                .collect::<Vec<_>>();
            HttpResponse::Ok().json(progress)
        }
        Err(error) => {
            log::error!(
                "failed to load daily quest progress {}: {:?}",
                user.id,
                error
            );
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

    match db.players.get_readonly(&user.id).await {
        Ok(player) => {
            let response: Vec<serde_json::Value> = player
                .achievements
                .into_iter()
                .map(|achievement| {
                    serde_json::json!({
                        "achievement_id": achievement.achievement_id,
                        "current_tier": achievement.current_tier,
                        "current_value": achievement.current_value,
                    })
                })
                .collect();

            HttpResponse::Ok().json(response)
        }
        Err(error) => {
            log::error!(
                "failed to load achievement progress {}: {:?}",
                user.id,
                error
            );
            HttpResponse::InternalServerError().finish()
        }
    }
}

async fn claim_story_reward(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    quest_id: web::Path<String>,
) -> HttpResponse {
    claim_reward(req, db, QuestClaimKind::Story, quest_id.into_inner()).await
}

async fn claim_daily_reward(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    quest_id: web::Path<String>,
) -> HttpResponse {
    claim_reward(req, db, QuestClaimKind::Daily, quest_id.into_inner()).await
}

async fn claim_reward(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
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
        Ok(reward) => HttpResponse::Ok().json(QuestClaimResp::from(reward)),
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
