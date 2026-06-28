use std::sync::Arc;

use actix::Addr;
use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::{
    api::get_session_token,
    config::GameConfig,
    database::{
        notifications::{set_badge, MODULE_INBOX, MODULE_QUESTS},
        DatabaseManager,
    },
    game::connection_mgr::{ConnectionManager, PushToUser},
    game::msg::{GameMsgOut, RewardEntry},
    logging::{ActivityEvent, ActivityEventKind, ActivityLogHandle},
};

// web::Data::from(Arc<GameConfig>) registers as web::Data<GameConfig>, not web::Data<Arc<GameConfig>>

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/complete_level", web::post().to(complete_level));
}

#[derive(Deserialize)]
struct CompleteLevelReq {
    mode: i32,
    level_id: i32,
    stars: i32,
}

#[derive(Serialize)]
struct CompleteLevelResp {
    ok: bool,
    new_level: i32,
    rewards: Vec<RewardEntry>,
}

async fn complete_level(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    game_config: web::Data<GameConfig>,
    activity_log: web::Data<ActivityLogHandle>,
    connection_mgr: web::Data<Addr<ConnectionManager>>,
    payload: web::Json<CompleteLevelReq>,
) -> HttpResponse {
    let Some(token) = get_session_token(&req) else {
        return HttpResponse::Unauthorized().finish();
    };
    let Some(user) = db.users.get_session_token(token, &db.friendships).await else {
        return HttpResponse::Unauthorized().finish();
    };

    let mode = payload.mode;
    let level_id = payload.level_id;
    let stars = payload.stars;

    if game_config.level_by_id(mode, level_id).is_none() {
        return HttpResponse::Ok().json(CompleteLevelResp { ok: false, new_level: 0, rewards: vec![] });
    }

    if let Err(e) = db.players.ensure_mode_progress(&user.id, &game_config).await {
        log::warn!("ensure_mode_progress failed for {}: {:?}", user.id, e);
    }

    let agg = match db.players.get_readonly(&user.id).await {
        Ok(a) => a,
        Err(_) => return HttpResponse::Ok().json(CompleteLevelResp { ok: false, new_level: 0, rewards: vec![] }),
    };
    let journey_progress = agg.mode_progress.get(&1).copied().unwrap_or(0);
    if let Some(mode_cfg) = game_config.mode_config(mode) {
        if mode_cfg.unlock_by_journey_level > 1 && journey_progress < mode_cfg.unlock_by_journey_level {
            return HttpResponse::Ok().json(CompleteLevelResp { ok: false, new_level: 0, rewards: vec![] });
        }
    } else {
        return HttpResponse::Ok().json(CompleteLevelResp { ok: false, new_level: 0, rewards: vec![] });
    }

    match db.players.complete_level(&user.id, mode, level_id, stars).await {
        Ok(outcome) => {
            activity_log.record(ActivityEvent::new(
                Some(user.id.to_string()),
                ActivityEventKind::CompleteLevel,
                Some(serde_json::json!({ "mode": mode, "level_id": level_id, "stars": stars })),
            ));

            // Push achievement unlocks
            for (achievement_id, tier) in &outcome.achievements {
                connection_mgr.do_send(PushToUser {
                    user_id: user.id.clone(),
                    msg: GameMsgOut::AchievementUnlocked {
                        achievement_id: achievement_id.clone(),
                        tier: *tier,
                    },
                });
            }

            // Progressive milestones → inbox badge + WS push (task 5.2)
            if !outcome.milestones.is_empty() {
                set_badge(&db.pool, &user.id, MODULE_INBOX).await;
                for (achievement_id, step, inbox_id) in &outcome.milestones {
                    connection_mgr.do_send(PushToUser {
                        user_id: user.id.clone(),
                        msg: GameMsgOut::ProgressiveMilestone {
                            achievement_id: achievement_id.clone(),
                            step: *step,
                        },
                    });
                    connection_mgr.do_send(PushToUser {
                        user_id: user.id.clone(),
                        msg: GameMsgOut::NewInboxMessage {
                            id: *inbox_id,
                            msg_type: "progressive_achievement".to_string(),
                            has_reward: true,
                        },
                    });
                }
            }

            // Quest completions → quests badge + WS push (task 6.2)
            if !outcome.completed_quests.is_empty() {
                set_badge(&db.pool, &user.id, MODULE_QUESTS).await;
                for (quest_id, quest_type) in &outcome.completed_quests {
                    connection_mgr.do_send(PushToUser {
                        user_id: user.id.clone(),
                        msg: GameMsgOut::QuestCompleted {
                            quest_id: quest_id.clone(),
                            quest_type: quest_type.clone(),
                        },
                    });
                }
            }

            let rewards = outcome.rewards.iter()
                .map(|(item_id, amount)| RewardEntry { item_id: item_id.clone(), amount: *amount })
                .collect();
            HttpResponse::Ok().json(CompleteLevelResp { ok: true, new_level: outcome.new_level, rewards })
        }
        Err(e) => {
            log::warn!("complete_level rejected user={} mode={} level_id={}: {:?}", user.id, mode, level_id, e);
            HttpResponse::Ok().json(CompleteLevelResp { ok: false, new_level: 0, rewards: vec![] })
        }
    }
}
