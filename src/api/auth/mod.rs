//! 匿名账号登录端点（设备绑定，免密）。
//!
//! POST /api/auth/anonymous  { "device_id": "<客户端生成的稳定 UUID>" }
//!
//! 复用 platform 免密建号模式（provider = "anon"）：首登自动建用户 +
//! auth_identities 行 + session；同一 device_id 复调幂等返回同账号（不改昵称），
//! 每次新建一条 session。返回 { user_id, session_token, username }。

use actix_web::{web, HttpResponse};
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{api::ApiResponse, database::DatabaseManager};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/anonymous", web::post().to(anonymous_login));
}

const DEVICE_ID_MAX_LEN: usize = 128;

#[derive(Deserialize)]
struct AnonymousReq {
    device_id: String,
}

#[derive(Serialize)]
struct AnonymousResp {
    user_id: String,
    session_token: String,
    username: String,
}

/// 生成形如「英歌侠1234」的昵称基。唯一性由 find_or_create_platform_user
/// 内部的 make_unique_username 兜底（碰撞追加后缀）。
fn gen_anon_nickname() -> String {
    let n: u16 = thread_rng().gen_range(0..=9999);
    format!("英歌侠{:04}", n)
}

async fn anonymous_login(
    db: web::Data<Arc<DatabaseManager>>,
    payload: web::Json<AnonymousReq>,
) -> HttpResponse {
    let device_id = payload.device_id.trim();
    if device_id.is_empty() || device_id.len() > DEVICE_ID_MAX_LEN {
        return HttpResponse::BadRequest().json(ApiResponse::new("invalid device_id"));
    }

    let nickname = gen_anon_nickname();
    match db
        .users
        .find_or_create_platform_user("anon", device_id, None, Some(&nickname), None)
        .await
    {
        Some((user_id, session_token)) => {
            // 回读 username：既有用户取其原昵称，新用户取刚生成（碰撞后）的唯一昵称。
            let username = db
                .users
                .get_id_public(user_id)
                .await
                .map(|u| u.username)
                .unwrap_or(nickname);
            HttpResponse::Ok().json(AnonymousResp {
                user_id: user_id.to_string(),
                session_token: session_token.to_string(),
                username,
            })
        }
        None => HttpResponse::InternalServerError()
            .json(ApiResponse::new("Failed to create or load anonymous user")),
    }
}
