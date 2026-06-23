use actix::Message;
use rand::{distributions::Alphanumeric, thread_rng, Rng};

#[derive(Clone)]
pub struct GameOId(String);

impl GameOId {
    pub fn new() -> Self {
        GameOId(
            thread_rng()
                .sample_iter(&Alphanumeric)
                .take(24)
                .map(char::from)
                .collect(),
        )
    }
}
pub enum GameEndReason {
    Regular,
    PlayerLeft,
    PlayerDisconnected,
}

pub enum GameLogEvent {
    StartGame { id: GameOId, ranked: bool },
    EndGame { id: GameOId, reason: GameEndReason },
}

impl Message for GameLogEvent {
    type Result = ();
}
