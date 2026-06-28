pub mod chat_msg;
pub mod friendships;
pub mod games;
pub mod inbox;
pub mod invites;
pub mod items;
pub mod leaderboard;
pub mod minigame_config;
pub mod minigame_leaderboard;
pub mod notifications;
pub mod quests;
pub mod users;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::player::PlayerRepository;

use self::{
    chat_msg::ChatMsgCollection, friendships::FriendshipCollection, games::GameCollection,
    invites::InviteCollection, items::ItemCollection, leaderboard::LeaderboardCollection,
    minigame_leaderboard::MinigameLeaderboardCollection, quests::QuestCollection,
    users::UserCollection,
};

const DATABASE_URL_DEFAULT: &str = "******localhost:5432/fourinarow";

pub struct DatabaseManager {
    pub pool: PgPool,
    pub users: UserCollection,
    pub games: GameCollection,
    pub friendships: FriendshipCollection,
    pub chat_msgs: ChatMsgCollection,
    pub items: ItemCollection,
    pub invites: InviteCollection,
    pub leaderboard: LeaderboardCollection,
    pub minigame_leaderboard: MinigameLeaderboardCollection,
    pub quests: QuestCollection,
    pub players: PlayerRepository,
}

impl DatabaseManager {
    pub async fn new() -> DatabaseManager {
        let url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| DATABASE_URL_DEFAULT.to_string());
        println!("Connecting to PostgreSQL at '{}'", url);

        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(&url)
            .await
            .expect("Failed to connect to PostgreSQL");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("Failed to run database migrations");

        let players = PlayerRepository::new(pool.clone());
        players.start_flush_worker();

        DatabaseManager {
            users: UserCollection::new(pool.clone()),
            games: GameCollection::new(pool.clone()),
            friendships: FriendshipCollection::new(pool.clone()),
            chat_msgs: ChatMsgCollection::new(pool.clone()),
            items: ItemCollection::new(pool.clone()),
            invites: InviteCollection::new(pool.clone()),
            leaderboard: LeaderboardCollection::new(pool.clone()),
            minigame_leaderboard: MinigameLeaderboardCollection::new(pool.clone()),
            quests: QuestCollection::new(pool.clone()),
            players,
            pool,
        }
    }
}
