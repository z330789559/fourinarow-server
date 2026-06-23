use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameEvent {
    GamePlayed,
    GameWon,
}

impl GameEvent {
    pub fn condition_type(&self) -> &'static str {
        match self {
            GameEvent::GamePlayed => "games_played",
            GameEvent::GameWon => "games_won",
        }
    }
}
