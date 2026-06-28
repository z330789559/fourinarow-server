//! Platform (WeChat / Douyin) mini-game login endpoints.
//!
//! # WeChat
//! POST /api/platform/wechat/login  { "code": "<wx.login js_code>", "nickname": "...", "avatar_url": "..." }
//!
//! # Douyin
//! POST /api/platform/douyin/login  { "code": "<tt.login code>", "nickname": "...", "avatar_url": "..." }
//!
//! Both endpoints exchange the platform one-time code for a server-side session
//! token using the respective platform's `jscode2session` API.  On first login a
//! new user account is created automatically.

use actix_web::{web, HttpResponse};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{api::ApiResponse, database::DatabaseManager};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/wechat").route("/login", web::post().to(wechat_login)))
        .service(web::scope("/douyin").route("/login", web::post().to(douyin_login)));
}

// ── shared request/response types ──────────────────────────────────────────

#[derive(Deserialize)]
struct PlatformLoginReq {
    code: String,
    nickname: Option<String>,
    /// Avatar URL; stored only for future use (not yet persisted to DB).
    #[allow(dead_code)]
    avatar_url: Option<String>,
}

#[derive(Serialize)]
struct PlatformLoginResp {
    session_token: String,
    user_id: String,
}

// ── WeChat ──────────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct WxCode2SessionResp {
    openid: Option<String>,
    unionid: Option<String>,
    session_key: Option<String>,
    errcode: Option<i32>,
    errmsg: Option<String>,
}

async fn wechat_login(
    db: web::Data<Arc<DatabaseManager>>,
    payload: web::Json<PlatformLoginReq>,
) -> HttpResponse {
    let appid = std::env::var("WECHAT_APPID").unwrap_or_default();
    let secret = std::env::var("WECHAT_SECRET").unwrap_or_default();

    if appid.is_empty() || secret.is_empty() {
        return HttpResponse::ServiceUnavailable().json(ApiResponse::new(
            "WeChat integration is not configured on this server",
        ));
    }

    let url = format!(
        "https://api.weixin.qq.com/sns/jscode2session\
         ?appid={}&secret={}&js_code={}&grant_type=authorization_code",
        appid, secret, payload.code
    );

    let wx_resp: WxCode2SessionResp = match Client::new().get(&url).send().await {
        Ok(r) => match r.json().await {
            Ok(v) => v,
            Err(e) => {
                log::error!("WeChat response parse error: {:?}", e);
                return HttpResponse::InternalServerError()
                    .json(ApiResponse::new("Failed to parse WeChat response"));
            }
        },
        Err(e) => {
            log::error!("WeChat API request error: {:?}", e);
            return HttpResponse::InternalServerError()
                .json(ApiResponse::new("Failed to call WeChat API"));
        }
    };

    if let Some(code) = wx_resp.errcode {
        if code != 0 {
            return HttpResponse::Unauthorized().json(ApiResponse::new(format!(
                "WeChat auth failed ({}): {}",
                code,
                wx_resp.errmsg.unwrap_or_default()
            )));
        }
    }

    let openid = match wx_resp.openid {
        Some(o) => o,
        None => {
            return HttpResponse::Unauthorized()
                .json(ApiResponse::new("WeChat did not return an openid"))
        }
    };

    match db
        .users
        .find_or_create_platform_user(
            "wechat",
            &openid,
            wx_resp.unionid.as_deref(),
            payload.nickname.as_deref(),
            wx_resp.session_key.as_deref(),
        )
        .await
    {
        Some((user_id, session_token)) => HttpResponse::Ok().json(PlatformLoginResp {
            session_token: session_token.to_string(),
            user_id: user_id.to_string(),
        }),
        None => HttpResponse::InternalServerError()
            .json(ApiResponse::new("Failed to create or load user")),
    }
}

// ── Douyin ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct DouyinCode2SessionReq<'a> {
    appid: &'a str,
    secret: &'a str,
    code: &'a str,
}

#[derive(Deserialize, Debug)]
struct DouyinCode2SessionResp {
    data: Option<DouyinSessionData>,
    message: Option<String>,
    err_no: Option<i32>,
}

#[derive(Deserialize, Debug)]
struct DouyinSessionData {
    openid: Option<String>,
    session_key: Option<String>,
}

async fn douyin_login(
    db: web::Data<Arc<DatabaseManager>>,
    payload: web::Json<PlatformLoginReq>,
) -> HttpResponse {
    let appid = std::env::var("DOUYIN_APPID").unwrap_or_default();
    let secret = std::env::var("DOUYIN_SECRET").unwrap_or_default();

    if appid.is_empty() || secret.is_empty() {
        return HttpResponse::ServiceUnavailable().json(ApiResponse::new(
            "Douyin integration is not configured on this server",
        ));
    }

    let body = DouyinCode2SessionReq {
        appid: &appid,
        secret: &secret,
        code: &payload.code,
    };

    let dy_resp: DouyinCode2SessionResp = match Client::new()
        .post("https://developer.toutiao.com/api/apps/v2/jscode2session")
        .json(&body)
        .send()
        .await
    {
        Ok(r) => match r.json().await {
            Ok(v) => v,
            Err(e) => {
                log::error!("Douyin response parse error: {:?}", e);
                return HttpResponse::InternalServerError()
                    .json(ApiResponse::new("Failed to parse Douyin response"));
            }
        },
        Err(e) => {
            log::error!("Douyin API request error: {:?}", e);
            return HttpResponse::InternalServerError()
                .json(ApiResponse::new("Failed to call Douyin API"));
        }
    };

    if let Some(err) = dy_resp.err_no {
        if err != 0 {
            return HttpResponse::Unauthorized().json(ApiResponse::new(format!(
                "Douyin auth failed ({}): {}",
                err,
                dy_resp.message.unwrap_or_default()
            )));
        }
    }

    let data = match dy_resp.data {
        Some(d) => d,
        None => {
            return HttpResponse::Unauthorized()
                .json(ApiResponse::new("Douyin did not return session data"))
        }
    };

    let openid = match data.openid {
        Some(o) => o,
        None => {
            return HttpResponse::Unauthorized()
                .json(ApiResponse::new("Douyin did not return an openid"))
        }
    };

    match db
        .users
        .find_or_create_platform_user(
            "douyin",
            &openid,
            None,
            payload.nickname.as_deref(),
            data.session_key.as_deref(),
        )
        .await
    {
        Some((user_id, session_token)) => HttpResponse::Ok().json(PlatformLoginResp {
            session_token: session_token.to_string(),
            user_id: user_id.to_string(),
        }),
        None => HttpResponse::InternalServerError()
            .json(ApiResponse::new("Failed to create or load user")),
    }
}
