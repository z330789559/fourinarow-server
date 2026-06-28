use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameEvent {
    GamePlayed,
    GameWon,
    LevelCompleted,
    ItemPurchased,
    InviteCreated,
}

impl GameEvent {
    pub fn condition_type(&self) -> &'static str {
        match self {
            GameEvent::GamePlayed => "games_played",
            GameEvent::GameWon => "games_won",
            GameEvent::LevelCompleted => "level_completed",
            GameEvent::ItemPurchased => "item_purchased",
            GameEvent::InviteCreated => "invite_created",
        }
    }
}
