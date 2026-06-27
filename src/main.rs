mod api;
mod config;
mod database;
mod game;
mod items;
mod logging;
mod player;
mod quests;

use std::io;
use std::sync::Arc;

use actix::Actor;
use actix_cors::Cors;
use actix_files as fs;
use actix_web::{middleware, web, App, HttpResponse, HttpServer};

use api::users::user_mgr::UserManager;
use config::GameConfig;
use database::DatabaseManager;
use dotenv::dotenv;
use game::connection_mgr::ConnectionManager;
use game::lobby_mgr::LobbyManager;
use logging::{ActivityLogHandle, Logger};

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:40146";

#[actix_web::main]
async fn main() {
    dotenv().expect("Failed to load .env file");

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "actix_web=info");
    }

    let bind_addr = if let Some(addr) = std::env::var("BIND").ok() {
        addr
    } else {
        DEFAULT_BIND_ADDR.to_string()
    };

    env_logger::builder()
        .format_timestamp_secs()
        .format_module_path(false)
        .init();

    let server = start_server(&bind_addr);

    let res = server.await;
    println!();
    match res {
        Ok(_) => println!("Server terminated cleanly"),
        Err(err) => println!("Server terminated with an error!.\nErr: {:?}", err,),
    }
}

async fn start_server(bind_addr: &str) -> io::Result<()> {
    println!("Running on {}.", bind_addr);

    // Fail-fast: load game config before anything else
    let game_config = Arc::new(GameConfig::load());
    println!("Game config loaded: {} modes", game_config.modes.len());

    let db_mgr = Arc::new(DatabaseManager::new().await);
    let activity_log = ActivityLogHandle::start(db_mgr.pool.clone());
    let logger_addr = Logger::new().start();
    let connection_mgr_addr =
        ConnectionManager::new(logger_addr.clone(), activity_log.clone()).start();
    let user_mgr_addr = UserManager::new(
        db_mgr.clone(),
        activity_log.clone(),
        connection_mgr_addr.clone(),
        game_config.clone(),
    )
    .start();
    let lobby_mgr_addr = LobbyManager::new(
        user_mgr_addr.clone(),
        connection_mgr_addr.clone(),
        logger_addr.clone(),
    )
    .start();

    let shutdown_db_mgr = db_mgr.clone();
    let shutdown_activity_log = activity_log.clone();
    let db_mgr = web::Data::new(db_mgr);
    let user_mgr_addr = web::Data::new(user_mgr_addr);
    let connection_mgr_addr = web::Data::new(connection_mgr_addr);
    let lobby_mgr_addr = web::Data::new(lobby_mgr_addr);
    let activity_log = web::Data::new(activity_log);
    let game_config = web::Data::from(game_config);

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            .app_data(db_mgr.clone())
            .app_data(lobby_mgr_addr.clone())
            .app_data(connection_mgr_addr.clone())
            .app_data(user_mgr_addr.clone())
            .app_data(activity_log.clone())
            .app_data(game_config.clone())
            .route(
                "/",
                web::get().to(|| async {
                    HttpResponse::Found()
                        .insert_header(("LOCATION", "/index.html"))
                        .finish()
                }),
            )
            .service(
                web::scope("/api")
                    .wrap(
                        Cors::default()
                            .allowed_origin("https://play.fourinarow.ffactory.me")
                            .allowed_origin("http://localhost")
                            // 开发：放行任意端口的 localhost / 127.0.0.1（Cocos 预览 :7456、浏览器预览等）。
                            // 上线正式域名时按需收紧；写接口仍受 admin token 保护。
                            .allowed_origin_fn(|origin, _req_head| {
                                origin.as_bytes().starts_with(b"http://localhost:")
                                    || origin.as_bytes().starts_with(b"http://127.0.0.1:")
                            })
                            .allowed_methods(vec!["GET", "POST", "DELETE"])
                            .allow_any_header()
                            .max_age(3600),
                    )
                    .configure(api::config),
            )
            .service(web::scope("/game").configure(|cfg| game::config(cfg)))
            .service(web::resource("/privacy").to(|| async {
                HttpResponse::Found()
                    .insert_header(("LOCATION", "/privacy.html"))
                    .finish()
            }))
            .service(
                fs::Files::new("/", "static/").default_handler(web::to(|| async {
                    HttpResponse::Found()
                        .insert_header(("LOCATION", "/404.html"))
                        .finish()
                })),
            )
            .default_service(web::to(HttpResponse::NotFound))
    })
    .keep_alive(std::time::Duration::from_secs(1))
    .bind(bind_addr)
    .expect("Failed to bind address.")
    .run()
    .await?;

    if let Err(error) = shutdown_db_mgr.players.flush_all().await {
        log::error!("failed to flush player cache during shutdown: {:?}", error);
    }

    // Flush any buffered activity log events before exit (task 2.5)
    shutdown_activity_log.force_flush();
    // Give the spawned flush task a moment to complete
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    Ok(())
}
