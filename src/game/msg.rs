use super::lobby_mgr::LobbyKind;
use super::{
    connection_mgr::WSSessionToken,
    game_info::{GameId, GAME_ID_LEN},
};
use crate::api::users::{session_token::SessionToken, user::UserId};
use actix::prelude::*;
use serde::{Deserialize, Serialize};

// ── Game JSON sub-protocol types (Task 5.1) ──────────────────────────────────

/// Uplink: client → server, prefixed with `G:` in the reliable layer
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GameMsgIn {
    StartGame,
    LevelPage { mode: i32, after_id: i32, page_size: usize },
    CompleteLevel { mode: i32, level_id: i32, stars: i32 },
}

/// Downlink: server → client, prefixed with `GP:` in the reliable layer
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GameMsgOut {
    FriendOnline { user_id: String },
    AchievementUnlocked { achievement_id: String, tier: i32 },
    LeaderboardUpdate { top: Vec<LeaderboardEntry> },
    StartGameResp { modes: Vec<ModeStatus>, levels: Vec<LevelEntry> },
    LevelPageResp { levels: Vec<LevelEntry>, has_more: bool },
    CompleteLevelResp { ok: bool, new_level: i32, rewards: Vec<RewardEntry> },
    QuestCompleted { quest_id: String, quest_type: String },
    ProgressiveMilestone { achievement_id: String, step: i32 },
    NewInboxMessage { id: i64, msg_type: String, has_reward: bool },
}

#[derive(Debug, Clone, Serialize)]
pub struct LeaderboardEntry {
    pub user_id: String,
    pub username: String,
    pub rank: i32,
    pub score: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModeStatus {
    pub mode: i32,
    pub unlocked: bool,
    pub current_level: i32,
    pub total_levels: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct LevelEntry {
    pub id: i32,
    pub mode: i32,
    pub best_stars: i32, // 0 = not yet completed; per-level star history not stored
    pub level_min: i32,
    pub level_max: i32,
    pub chapter: i32,
    pub ammo_count: i32,
    pub triple_star: i32,
    pub double_star: i32,
    pub single_star: i32,
    pub scene_id: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RewardEntry {
    pub item_id: String,
    pub amount: i32,
}

#[derive(Debug, Clone)]
pub enum ReliablePacketIn {
    Ack(usize),                // Acknowledge the server's message with that id
    Msg(usize, PlayerMessage), // Actual message with id and content
}

impl ReliablePacketIn {
    /// Parse a reliable packet from the client.
    /// Uses `splitn(3, "::")` so MSG content (e.g. JSON with "::") is preserved intact.
    pub fn parse(orig: &str) -> Result<ReliablePacketIn, ReliabilityError> {
        let parts: Vec<_> = orig.splitn(3, "::").collect();
        return if parts.len() == 2 && parts[0] == "ACK" {
            if let Ok(id) = parts[1].parse::<usize>() {
                Ok(ReliablePacketIn::Ack(id))
            } else {
                Err(ReliabilityError::InvalidFormat)
            }
        } else if parts.len() == 3 && parts[0] == "MSG" {
            if let Ok(id) = parts[1].parse::<usize>() {
                if let Some(player_msg) = PlayerMessage::parse(parts[2]) {
                    Ok(ReliablePacketIn::Msg(id, player_msg))
                } else {
                    Err(ReliabilityError::InvalidContent)
                }
            } else {
                Err(ReliabilityError::InvalidFormat)
            }
        } else {
            Err(ReliabilityError::UnknownMessage)
        };
    }
}

#[derive(Debug, Clone)]
pub enum ReliablePacketOut {
    Ack(usize), // Acknowledge the client's message with that id
    Msg {
        id: usize,
        msg: ServerMessage,
        retry_count: usize,
    },
    Err(ReliabilityError),
}

impl Message for ReliablePacketOut {
    type Result = ();
}

impl ReliablePacketOut {
    pub fn serialize(self) -> String {
        use ReliablePacketOut::*;
        match self {
            Ack(id) => format!("ACK::{}", id),
            Msg {
                id,
                msg,
                retry_count: _,
            } => format!("MSG::{}::{}", id, msg.serialize()),
            Err(err) => format!("ERR::{}", err.serialize()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ReliabilityError {
    InvalidContent, // Message content could not be parsed
    InvalidFormat,  // ReliableMessage could not be parsed
    UnknownMessage, // Correct format but unknown keyword (ack, syn, msg)
    #[allow(dead_code)]
    KillClient, // Sent in case client is fucking up bad. Kills it immediately.
}
impl ReliabilityError {
    pub fn serialize(self) -> String {
        use ReliabilityError::*;
        match self {
            InvalidContent => "INVALID_CONTENT",
            InvalidFormat => "INVALID_FORMAT",
            UnknownMessage => "UNKNOWN_MESSAGE",
            KillClient => "KILL_CLIENT",
            //Unknown => "UNKNOWN",
        }
        .into()
    }
}

impl Into<ReliablePacketOut> for ReliabilityError {
    fn into(self) -> ReliablePacketOut {
        ReliablePacketOut::Err(self)
    }
}

pub struct HelloIn {
    pub protocol_version: usize,
    pub maybe_session_token: Option<WSSessionToken>,
}

impl HelloIn {
    pub fn parse(orig: &str) -> Option<Self> {
        //let uppercase = orig.to_uppercase();
        let parts: Vec<_> = orig.split("::").collect();
        if parts.len() == 3 && parts[0] == "HELLO" {
            if let Ok(protocol_version) = parts[1].parse() {
                let request_parts: Vec<_> = parts[2].split(":").collect();
                let maybe_session_token = if request_parts.len() == 1 && request_parts[0] == "NEW" {
                    None
                } else if request_parts.len() == 2 && request_parts[0] == "REQ" {
                    let session_token = request_parts[1].to_string();
                    if session_token.len() != 32 {
                        None
                    } else {
                        Some(session_token)
                    }
                } else {
                    None
                };
                return Some(HelloIn {
                    protocol_version,
                    maybe_session_token,
                });
            }
        }
        None
    }
}

pub enum HelloOut {
    Ok(WSSessionToken, bool), // Hello sessionToken and session_token.is_new
    OutDated,                 // Sent when the client's version is too old
}
impl HelloOut {
    pub fn serialize(self) -> String {
        use HelloOut::*;
        match self {
            Ok(session_token, is_new) => {
                let is_new_str = if is_new { "NEW" } else { "FOUND" };
                format!("HELLO::{}::{}", is_new_str, session_token)
            }
            OutDated => "HELLO::OUTDATED".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ServerMessage {
    PlaceChip(usize),
    OpponentLeaving,
    OpponentJoining,
    GameStart(bool, Option<String>),
    GameOver(bool), // true if recipient won
    LobbyClosing,
    ReadyForGamePing,
    LoginResponse {
        success: bool,
    },
    Error(Option<SrvMsgError>),
    BattleReq(UserId, GameId),
    CurrentServerState(usize, bool), // connected players, someone wants to play
    ChatMessage(bool, String, Option<String>), // is_global, message, sender_name
    ChatRead(bool),                  // is_global

    /// Sent when another client logs into an account which is authenticated on this connection -> this one is closed
    CloseOtherClientLogin,
    /// Game JSON sub-protocol push/response (Task 5.1); serialized as `GP:{json}`
    GameProtocol(GameMsgOut),
}

impl ServerMessage {
    pub fn serialize(&self) -> String {
        use ServerMessage::*;
        match self.clone() {
            PlaceChip(row) => format!("PC:{}", row),
            OpponentLeaving => "OPP_LEAVING".to_owned(),
            OpponentJoining => "OPP_JOINED".to_owned(),
            GameStart(your_turn, maybe_username) => format!(
                "GAME_START:{}{}",
                if your_turn { "YOU" } else { "OPP" },
                if let Some(username) = maybe_username {
                    format!(":{}", username)
                } else {
                    "".to_owned()
                }
            ),
            GameOver(you_win) => format!("GAME_OVER:{}", if you_win { "YOU" } else { "OPP" }),
            LobbyClosing => "LOBBY_CLOSING".to_owned(),
            ReadyForGamePing => "READY_FOR_GAME_PING".to_owned(),
            LoginResponse { success } => format!("LOGIN_RESPONSE:{}", success.to_string()),
            Error(maybe_msg) => {
                if let Some(msg) = maybe_msg {
                    format!("ERROR:{}", msg.serialize())
                } else {
                    "ERROR".to_owned()
                }
            }
            BattleReq(requesting_id, lobby_id) => {
                format!("BATTLE_REQ:{}:{}", requesting_id, lobby_id)
            }
            CurrentServerState(connected_players, player_waiting) => format!(
                "CURRENT_SERVER_STATE:{}:{}",
                connected_players, player_waiting
            ),
            ChatMessage(is_global, message, maybe_sender) => {
                let sender_name = if let Some(sender) = maybe_sender {
                    sender
                } else {
                    "".to_string()
                };
                let encoded_message = base64::encode_config(message, base64::STANDARD);
                format!("CHAT_MSG:{}:{}:{}", is_global, encoded_message, sender_name)
            }
            ChatRead(is_global) => format!("CHAT_READ:{}", is_global),
            CloseOtherClientLogin => "CLOSE_OTHER_CLIENT_LOGIN".to_owned(),
            GameProtocol(msg) => format!(
                "GP:{}",
                serde_json::to_string(&msg).unwrap_or_else(|_| "{}".to_string())
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SrvMsgError {
    Internal,
    // InvalidMessage,
    LobbyNotFound,
    LobbyFull,
    InvalidColumn,
    NotInLobby,
    NotYourTurn,
    AlreadyInLobby,
    GameNotStarted,
    GameNotOver,
    NotLoggedIn,
    UserNotPlaying,
    NoSuchUser,
}

impl SrvMsgError {
    fn serialize(self) -> String {
        use SrvMsgError::*;
        match self {
            Internal => "Internal".to_owned(),
            NotYourTurn => "NotYourTurn".to_owned(),
            NotInLobby => "NotInLobby".to_owned(),
            LobbyNotFound => "LobbyNotFound".to_owned(),
            LobbyFull => "LobbyFull".to_owned(),
            InvalidColumn => "InvalidColumn".to_owned(),
            GameNotStarted => "GameNotStarted".to_owned(),
            AlreadyInLobby => "AlreadyInLobby".to_owned(),
            GameNotOver => "GameNotOver".to_owned(),
            NotLoggedIn => "NotLoggedIn".to_owned(),
            UserNotPlaying => "UserNotPlaying".to_owned(),
            NoSuchUser => "NoSuchUser".to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum PlayerMessage {
    PlaceChip(usize),
    PlayAgainRequest,
    Leaving,
    ReadyForGamePong,
    LobbyRequest(LobbyKind),
    LobbyJoin(GameId),
    Login(SessionToken),
    Logout,
    BattleReq(UserId),
    ChatMessage(String),
    ChatRead,
    /// Game JSON sub-protocol uplink (Task 5.1); parsed from `G:{json}`
    GameProtocol(GameMsgIn),
}

impl PlayerMessage {
    pub fn parse(orig: &str) -> Option<PlayerMessage> {
        // G: prefix check on original string BEFORE to_uppercase() — JSON must not be uppercased
        if orig.starts_with("G:") {
            return serde_json::from_str::<GameMsgIn>(&orig[2..])
                .ok()
                .map(PlayerMessage::GameProtocol);
        }

        let s = orig.to_uppercase();
        if s.len() > 1000 {
            // chat messages might be long
            return None;
        }
        use PlayerMessage::*;
        if s.starts_with("PC:") && s.len() == 4 {
            if let Ok(row) = s[3..4].parse() {
                return Some(PlaceChip(row));
            }
        } else if s == "REQ_LOBBY" {
            return Some(LobbyRequest(LobbyKind::Private));
        } else if s == "REQ_WW" {
            return Some(LobbyRequest(LobbyKind::Public));
        // } else if s == "PlayerLeaving" {
        //     return Some(PlayerLeaving);
        } else if s.starts_with("JOIN_LOBBY:") && s.len() == 11 + GAME_ID_LEN {
            if let Some(id) = GameId::parse(&s[11..11 + GAME_ID_LEN]) {
                return Some(LobbyJoin(id));
            }
        } else if s == "LEAVE" {
            return Some(Leaving);
        } else if s == "READY_FOR_GAME_PONG" {
            return Some(ReadyForGamePong);
        } else if s == "PLAY_AGAIN" {
            return Some(PlayAgainRequest);
        } else if s.starts_with("LOGIN:") {
            let split: Vec<&str> = orig.split(":").collect();
            if split.len() == 2 {
                return Some(Login(SessionToken::parse(split[1])));
            }
        } else if s == "LOGOUT" {
            return Some(Logout);
        } else if s.starts_with("BATTLE_REQ") {
            let split: Vec<&str> = orig.split(':').collect();
            if split.len() == 2 {
                if let Ok(user_id) = UserId::from_str(&split[1]) {
                    return Some(BattleReq(user_id));
                }
                // Err(e) => println!("Error: invalid battlereq userid ({})", e),
                // }
            }
        } else if s.starts_with("CHAT_MSG") {
            let split: Vec<&str> = orig.split(':').collect();
            if split.len() == 2 {
                if let Ok(Ok(decoded_msg)) =
                    base64::decode_config(split[1], base64::STANDARD).map(String::from_utf8)
                {
                    return Some(ChatMessage(decoded_msg));
                }
            }
        } else if s == "CHAT_READ" {
            return Some(ChatRead);
        }
        None
    }
}

impl Message for PlayerMessage {
    type Result = Result<(), ()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ① Normal text command: MSG::1::PC:3 → PlaceChip(3)
    #[test]
    fn splitn_pc_3() {
        match ReliablePacketIn::parse("MSG::1::PC:3") {
            Ok(ReliablePacketIn::Msg(1, PlayerMessage::PlaceChip(3))) => {}
            other => panic!("expected Msg(1, PlaceChip(3)), got {:?}", other),
        }
    }

    /// ② Chat message (base64, contains single colon): MSG::1::CHAT_MSG:<b64>
    #[test]
    fn splitn_chat_msg() {
        let encoded = base64::encode_config("hello", base64::STANDARD);
        let input = format!("MSG::1::CHAT_MSG:{}", encoded);
        match ReliablePacketIn::parse(&input) {
            Ok(ReliablePacketIn::Msg(1, PlayerMessage::ChatMessage(ref s))) if s == "hello" => {}
            other => panic!("expected ChatMessage(hello), got {:?}", other),
        }
    }

    /// ③ JSON game message: MSG::1::G:{"kind":"start_game"}
    #[test]
    fn splitn_json_start_game() {
        match ReliablePacketIn::parse(r#"MSG::1::G:{"kind":"start_game"}"#) {
            Ok(ReliablePacketIn::Msg(1, PlayerMessage::GameProtocol(GameMsgIn::StartGame))) => {}
            other => panic!("expected GameProtocol(StartGame), got {:?}", other),
        }
    }

    /// ④ JSON content that itself contains "::": splitn must preserve it intact
    #[test]
    fn splitn_json_content_double_colon_preserved() {
        let input = r#"MSG::1::G:{"kind":"level_page","mode":1,"after_id":0,"page_size":10}"#;
        // verify parts are split correctly at the first two "::" only
        let parts: Vec<_> = input.splitn(3, "::").collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "MSG");
        assert_eq!(parts[1], "1");
        assert!(parts[2].starts_with("G:"));

        // also verify a string with :: inside the JSON body isn't mangled
        let with_colons = r#"MSG::2::G:{"kind":"start_game"}"#;
        // replace "start_game" with a fake value containing :: to test the guarantee
        let raw = r#"MSG::2::G:{"data":"a::b","kind":"start_game"}"#;
        let raw_parts: Vec<_> = raw.splitn(3, "::").collect();
        assert_eq!(raw_parts[2], r#"G:{"data":"a::b","kind":"start_game"}"#,
            ":: inside JSON content must be preserved by splitn(3)");
        // suppress unused variable warning for `with_colons`
        let _ = with_colons;
    }
}
// impl Message for ServerMessageNamed {
//     type Result = Result<(), ()>; // whether action was successful or not
// }
impl Message for ServerMessage {
    type Result = Result<(), ()>; // whether action was successful or not
}
