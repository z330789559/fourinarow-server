pub mod auth;
pub mod chat;
mod feedback;
pub mod gameplay;
pub mod inbox;
pub mod inventory;
pub mod invites;
pub mod leaderboard;
pub mod minigame_config;
pub mod minigame_leaderboard;
pub mod minigame_tasks;
pub mod notifications;
pub mod platform;
pub mod quests;
pub mod users;

use actix_web::{web, HttpRequest, HttpResponse, HttpResponseBuilder};
use serde::Serialize;
use HttpResponse as HR;

use self::users::session_token::SessionToken;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/")
            .route(web::get().to(HttpResponse::Ok))
            .route(web::head().to(HttpResponse::MethodNotAllowed)),
    )
    .service(web::scope("/users").configure(users::config))
    .service(web::scope("/auth").configure(auth::config))
    .service(web::scope("/chat").configure(chat::config))
    .service(web::scope("/feedback").configure(feedback::config))
    .service(web::scope("/platform").configure(platform::config))
    .service(web::scope("/leaderboard").configure(leaderboard::config))
    .service(web::scope("/invites").configure(invites::config))
    .service(web::scope("/inventory").configure(inventory::config))
    .service(web::scope("/quests").configure(quests::config))
    .service(web::scope("/game").configure(gameplay::config))
    .service(web::scope("/notifications").configure(notifications::config))
    .service(web::scope("/inbox").configure(inbox::config))
    .service(web::scope("/minigame-config").configure(minigame_config::config))
    .service(
        web::scope("/minigame")
            .configure(minigame_leaderboard::config)
            .configure(minigame_tasks::config),
    );
}

#[derive(Serialize)]
pub struct ApiResponse<T> {
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<T>,
}
impl ApiResponse<()> {
    pub fn new<T: Into<String>>(message: T) -> Self {
        ApiResponse {
            message: message.into(),
            content: None,
        }
    }
    #[allow(unreachable_patterns)]
    pub fn from(err: ApiError) -> HttpResponse {
        let prefix = String::from("Error: ");

        let (http_response, description): (fn() -> HttpResponseBuilder, &str) = match err {
            ApiError::PasswordInsufficient => (HR::BadRequest, "insufficient password"),
            ApiError::EmailInUse => (HR::BadRequest, "email in use"),
            ApiError::UsernameInUse => (HR::BadRequest, "username in use"),
            ApiError::InvalidUsername => (
                HR::BadRequest,
                "username invalid (too short, long or containing invalid characters)",
            ),

            ApiError::AlreadyPlaying => (HR::BadRequest, "user is already playing"),
            ApiError::MissingSessionToken => (HR::Unauthorized, "missing header session_token"),
            ApiError::IncorrectCredentials => (HR::Forbidden, "the credentials are incorrect"),
            ApiError::InternalServerError => (HR::InternalServerError, "internal server error"),
        };
        http_response().json(ApiResponse::new(prefix + description))
    }
}

impl<T> ApiResponse<T> {
    #[allow(dead_code)]
    pub fn with_content(message: &str, content: T) -> Self {
        ApiResponse {
            message: message.to_owned(),
            content: Some(content),
        }
    }
}

#[allow(dead_code)]
#[non_exhaustive]
pub enum ApiError {
    UsernameInUse,
    EmailInUse,
    PasswordInsufficient,
    InvalidUsername,
    IncorrectCredentials,
    AlreadyPlaying,
    MissingSessionToken,
    InternalServerError,
}

pub fn get_session_token(req: &HttpRequest) -> Option<SessionToken> {
    req.headers()
        .get("SessionToken")
        .and_then(|value| value.to_str().ok())
        .map(SessionToken::parse)
}
