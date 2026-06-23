#![allow(dead_code)]
use actix::Message;
use rand::{distributions::Alphanumeric, thread_rng, Rng};

#[derive(Clone)]
pub struct LobbyId(String);

impl LobbyId {
    pub fn new() -> Self {
        LobbyId(
            thread_rng()
                .sample_iter(&Alphanumeric)
                .take(24)
                .map(char::from)
                .collect(),
        )
    }
}

pub enum LobbyCloseReason {
    Cancel,                        // Player waited but noone joined -> cancelled
    Success { games_played: u32 }, // Lobby had at least 1 game played
}

pub enum LobbyLogEvent {
    LobbyCreated {
        id: LobbyId,
    },
    LobbyClosed {
        id: LobbyId,
        // reason: LobbyCloseReason,
    },
}

impl Message for LobbyLogEvent {
    type Result = ();
}
