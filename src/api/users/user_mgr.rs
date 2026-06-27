use crate::api::{chat::ChatThreadId, users::user::*, ApiError};
use crate::config::GameConfig;
use crate::database::{notifications::{set_badge, MODULE_INBOX, MODULE_QUESTS}, DatabaseManager};
use crate::game::client_adapter::ClientAdapterMsg;
use crate::game::connection_mgr::{ConnectionManager, PushToUser};
use crate::game::lobby_mgr::{self, LobbyManager};
use crate::game::msg::*;
use crate::logging::{ActivityEvent, ActivityEventKind, ActivityLogHandle};

use actix::prelude::*;
use serde::Deserialize;
use std::sync::Arc;
use serde_json;

const SR_PER_WIN: i32 = 25;

pub struct UserManager {
    db: Arc<DatabaseManager>,
    activity_log: ActivityLogHandle,
    connection_mgr: Addr<ConnectionManager>,
    game_config: Arc<GameConfig>,
    lobby_mgr_state: BacklinkState,
}
impl UserManager {
    pub fn new(
        db: Arc<DatabaseManager>,
        activity_log: ActivityLogHandle,
        connection_mgr: Addr<ConnectionManager>,
        game_config: Arc<GameConfig>,
    ) -> UserManager {
        UserManager {
            db,
            activity_log,
            connection_mgr,
            game_config,
            lobby_mgr_state: BacklinkState::Waiting,
        }
    }
}

#[derive(Clone)]
enum BacklinkState {
    Waiting,
    Linked(Addr<LobbyManager>),
}

impl Actor for UserManager {
    type Context = Context<Self>;
}

#[derive(Deserialize, Debug)]
pub struct UserAuth {
    pub username: String,
    pub password: String,
}

pub mod msg {

    use std::time::Duration;

    use futures::future::OptionFuture;

    use super::*;
    use crate::{
        api::users::session_token::SessionToken,
        game::{client_state::ClientState, msg::SrvMsgError},
    };

    pub struct Register(pub UserAuth);
    impl Message for Register {
        type Result = Result<SessionToken, ApiError>;
    }
    impl Handler<Register> for UserManager {
        type Result = ResponseActFuture<Self, Result<SessionToken, ApiError>>;

        fn handle(&mut self, msg: Register, _ctx: &mut Self::Context) -> Self::Result {
            let auth = msg.0;
            let db = self.db.clone();
            let activity_log = self.activity_log.clone();
            let game_config = self.game_config.clone();

            Box::pin(
                async move {
                    let username_is_in_use = db
                        .users
                        .get_username(&auth.username, &db.friendships)
                        .await
                        .is_some();

                    if !BackendUserMe::check_password(&auth.password) {
                        Err(ApiError::PasswordInsufficient)
                    } else if username_is_in_use {
                        Err(ApiError::UsernameInUse)
                    } else {
                        let mut user =
                            BackendUserMe::new(auth.username.clone(), auth.password.clone());
                        while db.users.get_id(&user.id, &db.friendships).await.is_some() {
                            user.gen_new_id();
                        }
                        let new_user_id = user.id;
                        let user_id_str = new_user_id.to_string();
                        db.users.insert(user.clone()).await;
                        let result = db.users
                            .create_session_token(auth, &db.friendships)
                            .await
                            .map(|(token, _)| token)
                            .ok_or(ApiError::IncorrectCredentials);
                        if result.is_ok() {
                            // Task 5.3: initialize mode progress for new user; failure tolerated
                            if let Err(e) = db.players
                                .ensure_mode_progress(&new_user_id, &game_config)
                                .await
                            {
                                log::warn!(
                                    "init mode progress failed for {}: {:?}",
                                    new_user_id,
                                    e
                                );
                            }
                            activity_log.record(ActivityEvent::new(
                                Some(user_id_str),
                                ActivityEventKind::Register,
                                None,
                            ));
                        }
                        result
                    }
                }
                .into_actor(self),
            )
        }
    }
    pub struct Login(pub UserAuth);
    impl Message for Login {
        type Result = Result<SessionToken, ApiError>;
    }
    impl Handler<Login> for UserManager {
        type Result = ResponseActFuture<Self, Result<SessionToken, ApiError>>;

        fn handle(&mut self, msg: Login, _ctx: &mut Self::Context) -> Self::Result {
            let db = self.db.clone();
            let activity_log = self.activity_log.clone();
            Box::pin(
                async move {
                    let pair = db.users
                        .create_session_token(msg.0, &db.friendships)
                        .await;
                    if let Some((_, ref user_id)) = pair {
                        activity_log.record(ActivityEvent::new(
                            Some(user_id.clone()),
                            ActivityEventKind::Login,
                            None,
                        ));
                    }
                    pair.map(|(token, _)| token).ok_or(ApiError::IncorrectCredentials)
                }
                .into_actor(self),
            )
        }
    }

    pub struct Logout(pub SessionToken);
    impl Message for Logout {
        type Result = Result<(), ApiError>;
    }
    impl Handler<Logout> for UserManager {
        type Result = ResponseActFuture<Self, Result<(), ApiError>>;

        fn handle(&mut self, msg: Logout, _ctx: &mut Self::Context) -> Self::Result {
            let db = self.db.clone();
            let activity_log = self.activity_log.clone();
            Box::pin(
                async move {
                    let user_id_str = if let Some(user) = db
                        .users
                        .get_session_token(msg.0.clone(), &db.friendships)
                        .await
                    {
                        if let Err(error) = db.players.flush_player(&user.id).await {
                            log::error!(
                                "failed to flush player {} on logout: {:?}",
                                user.id,
                                error
                            );
                            return Err(ApiError::InternalServerError);
                        }
                        Some(user.id.to_string())
                    } else {
                        None
                    };
                    let result = db.users
                        .remove_session_token(msg.0)
                        .await
                        .map_err(|_| ApiError::InternalServerError);
                    if result.is_ok() {
                        activity_log.record(ActivityEvent::new(
                            user_id_str,
                            ActivityEventKind::Logout,
                            None,
                        ));
                    }
                    result
                }
                .into_actor(self),
            )
        }
    }

    pub struct StartPlaying {
        pub session_token: SessionToken,
        pub addr: Addr<ClientState>,
    }
    impl Message for StartPlaying {
        type Result = Result<PublicUserMe, ()>;
    }
    struct StartPlayingIntermediate {
        result: Result<PublicUserMe, ()>,
        client_adapter_addr_to_close: Option<Addr<ClientState>>,
    }
    impl Handler<StartPlaying> for UserManager {
        type Result = ResponseActFuture<Self, Result<PublicUserMe, ()>>;
        fn handle(&mut self, msg: StartPlaying, _ctx: &mut Self::Context) -> Self::Result {
            let db = self.db.clone();
            Box::pin(
                async move {
                    if let Some(user) = db
                        .users
                        .get_session_token(msg.session_token, &db.friendships)
                        .await
                    {
                        let client_adapter_addr_to_close = if let Some(addr) = user.playing.clone()
                        {
                            if addr != msg.addr {
                                // Close other client's connection due to a new one logging in
                                addr.do_send(ServerMessage::CloseOtherClientLogin);
                                Some(addr)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        let mut user = user;
                        user.playing = Some(msg.addr);
                        db.users.update(user.clone()).await;
                        StartPlayingIntermediate {
                            result: Ok(user.to_public_user_me(&db).await),
                            client_adapter_addr_to_close,
                        }
                    } else {
                        StartPlayingIntermediate {
                            result: Err(()),
                            client_adapter_addr_to_close: None,
                        }
                    }
                }
                .into_actor(self)
                .map(|res, _act, ctx| {
                    if let Some(addr) = res.client_adapter_addr_to_close {
                        ctx.run_later(Duration::from_millis(100), move |_, _| {
                            addr.do_send(ClientAdapterMsg::Close);
                        });
                    }
                    res.result
                }),
            )
        }
    }

    pub enum IntUserMgrMsg {
        Backlink(Addr<LobbyManager>),
        Game(GameMsg),
        // StartPlaying(String, String),
        StopPlaying(UserId, Addr<ClientState>),
    }
    pub enum GameMsg {
        PlayedGame(PlayedGameInfo),
    }

    impl Message for IntUserMgrMsg {
        type Result = ();
    }
    impl Handler<IntUserMgrMsg> for UserManager {
        type Result = ResponseActFuture<Self, ()>;
        fn handle(&mut self, msg: IntUserMgrMsg, _ctx: &mut Self::Context) -> Self::Result {
            let db = self.db.clone();
            let connection_mgr = self.connection_mgr.clone();
            Box::pin(
                async move {
                    use GameMsg::*;
                    use IntUserMgrMsg::*;
                    let mut lobby_mgr_state: Option<BacklinkState> = None;
                    match msg {
                        Backlink(lobby_mgr) => {
                            lobby_mgr_state = Some(BacklinkState::Linked(lobby_mgr))
                        }
                        Game(game_msg) => match game_msg {
                            PlayedGame(game_info) => {
                                let settlement_id = game_info.settlement_id;
                                let winner_id = game_info.winner;
                                let loser_id = game_info.loser;
                                let settle_result = db
                                    .players
                                    .settle_game(&settlement_id, &winner_id, &loser_id, SR_PER_WIN)
                                    .await;
                                db.users.invalidate_cache(&winner_id);
                                db.users.invalidate_cache(&loser_id);
                                match settle_result {
                                    Ok(outcome) => {
                                        // Push achievement unlocks to winner
                                        for (achievement_id, tier) in &outcome.winner_achievements {
                                            connection_mgr.do_send(PushToUser {
                                                user_id: winner_id,
                                                msg: GameMsgOut::AchievementUnlocked {
                                                    achievement_id: achievement_id.clone(),
                                                    tier: *tier,
                                                },
                                            });
                                        }
                                        // Push quest completions (task 6.3)
                                        if !outcome.winner_completed_quests.is_empty() {
                                            set_badge(&db.pool, &winner_id, MODULE_QUESTS).await;
                                            for (quest_id, quest_type) in &outcome.winner_completed_quests {
                                                connection_mgr.do_send(PushToUser {
                                                    user_id: winner_id,
                                                    msg: GameMsgOut::QuestCompleted {
                                                        quest_id: quest_id.clone(),
                                                        quest_type: quest_type.clone(),
                                                    },
                                                });
                                            }
                                        }
                                        if !outcome.loser_completed_quests.is_empty() {
                                            set_badge(&db.pool, &loser_id, MODULE_QUESTS).await;
                                            for (quest_id, quest_type) in &outcome.loser_completed_quests {
                                                connection_mgr.do_send(PushToUser {
                                                    user_id: loser_id,
                                                    msg: GameMsgOut::QuestCompleted {
                                                        quest_id: quest_id.clone(),
                                                        quest_type: quest_type.clone(),
                                                    },
                                                });
                                            }
                                        }
                                        // Task 4.4: push leaderboard update to top-20 online players
                                        let top = db.leaderboard.get_top_by_rating(20, 0).await;
                                        let lb_entries: Vec<LeaderboardEntry> = top
                                            .iter()
                                            .map(|e| LeaderboardEntry {
                                                user_id: e.user_id.clone(),
                                                username: e.username.clone(),
                                                rank: e.rank as i32,
                                                score: e.skill_rating,
                                            })
                                            .collect();
                                        for entry in &top {
                                            if let Ok(uid) = UserId::from_str(&entry.user_id) {
                                                connection_mgr.do_send(PushToUser {
                                                    user_id: uid,
                                                    msg: GameMsgOut::LeaderboardUpdate {
                                                        top: lb_entries.clone(),
                                                    },
                                                });
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        log::error!(
                                            "failed to settle game settlement_id={} winner={} loser={}: {:?}",
                                            settlement_id,
                                            winner_id,
                                            loser_id,
                                            error
                                        );
                                    }
                                }
                            }
                        },
                        // StartPlaying(id) => {
                        //     if let Some(user) = db.users.get_mut(&id) {
                        //         user.playing = false;
                        //     }
                        // }
                        StopPlaying(id, addr) => {
                            if let Some(mut user) = db.users.get_id(&id, &db.friendships).await {
                                if let Some(playing_addr) = user.playing {
                                    if playing_addr == addr {
                                        // Only reset the address if the requesting ClientAdapter is still linked (might have been replaced already)
                                        user.playing = None;
                                        db.users.update(user).await;
                                        if let Err(error) = db.players.flush_player(&id).await {
                                            log::error!(
                                                "failed to flush player {} on stop playing: {:?}",
                                                id,
                                                error
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    lobby_mgr_state
                }
                .into_actor(self)
                .map(|maybe_lobby_mgr_state, act, _| {
                    if let Some(state) = maybe_lobby_mgr_state {
                        act.lobby_mgr_state = state;
                    }
                }),
            )
        }
    }

    pub struct SearchUsers {
        pub query: String,
    }
    impl Message for SearchUsers {
        type Result = Option<Vec<PublicUserOther>>;
    }

    impl Handler<SearchUsers> for UserManager {
        type Result = ResponseActFuture<Self, Option<Vec<PublicUserOther>>>;

        fn handle(&mut self, msg: SearchUsers, _ctx: &mut Self::Context) -> Self::Result {
            let db = self.db.clone();
            Box::pin(async move { Some(db.users.query(&msg.query).await) }.into_actor(self))
        }
    }

    pub struct GetUserMe(pub SessionToken);
    impl Message for GetUserMe {
        type Result = Option<PublicUserMe>;
    }

    impl Handler<GetUserMe> for UserManager {
        type Result = ResponseActFuture<Self, Option<PublicUserMe>>;
        fn handle(&mut self, msg: GetUserMe, _ctx: &mut Self::Context) -> Self::Result {
            let db = self.db.clone();
            Box::pin(
                async move {
                    Into::<OptionFuture<_>>::into(
                        db.users
                            .get_session_token(msg.0, &db.friendships)
                            .await
                            .map(|user| user.to_public_user_me(&db)),
                    )
                    .await
                }
                .into_actor(self),
            )
        }
    }

    pub struct GetUserOther(pub UserId);
    impl Message for GetUserOther {
        type Result = Option<PublicUserOther>;
    }

    impl Handler<GetUserOther> for UserManager {
        type Result = ResponseActFuture<Self, Option<PublicUserOther>>;
        fn handle(&mut self, msg: GetUserOther, _ctx: &mut Self::Context) -> Self::Result {
            let db = self.db.clone();
            Box::pin(async move { db.users.get_id_public(msg.0).await }.into_actor(self))
        }
    }

    pub struct UserAction {
        pub action: Action,
        pub session_token: SessionToken,
    }
    pub enum Action {
        FriendsAction(FriendsAction),
    }
    pub enum FriendsAction {
        Request(UserId),
        Delete(UserId), // will delete either a friend or an outgoing or incoming friend request
    }
    impl Message for UserAction {
        type Result = bool;
    }
    impl Handler<UserAction> for UserManager {
        type Result = ResponseActFuture<Self, bool>;
        fn handle(&mut self, msg: UserAction, _ctx: &mut Self::Context) -> Self::Result {
            let db = self.db.clone();
            let activity_log = self.activity_log.clone();
            Box::pin(
                async move {
                    if let Some(user_me) = db
                        .users
                        .get_session_token(msg.session_token, &db.friendships)
                        .await
                    {
                        match msg.action {
                            Action::FriendsAction(friends_action) => {
                                use FriendsAction::*;
                                match friends_action {
                                    Request(other_id) => {
                                        // If not trying to add myself && isn't already friend
                                        if user_me.id != other_id
                                            && !user_me
                                                .friendships
                                                .friends()
                                                .any(|f| f.other_id == other_id)
                                        {
                                            let updated = if user_me.friendships.iter().any(|req| {
                                                req.state == BackendFriendshipState::ReqIncoming
                                                    && req.other_id == other_id
                                            }) {
                                                // User has incoming friend request from other user -> accept request
                                                let chat_thread_id = ChatThreadId::new();
                                                db.friendships
                                                    .upgrade_to_friends(
                                                        user_me.id,
                                                        other_id,
                                                        chat_thread_id,
                                                    )
                                                    .await
                                            } else if user_me.friendships.iter().any(|req| {
                                                req.state == BackendFriendshipState::ReqOutgoing
                                                    && req.other_id == other_id
                                            }) {
                                                // User has already sent a request to this user.
                                                true
                                            } else {
                                                db.friendships.insert(user_me.id, other_id).await
                                            };

                                            if updated {
                                                db.users.invalidate_cache(&user_me.id);
                                                db.users.invalidate_cache(&other_id);
                                                activity_log.record(ActivityEvent::new(
                                                    Some(user_me.id.to_string()),
                                                    ActivityEventKind::AddFriend,
                                                    Some(serde_json::json!({ "other_id": other_id.to_string() })),
                                                ));
                                            }
                                            updated
                                        } else {
                                            false
                                        }
                                    }
                                    Delete(other_id) => {
                                        if user_me
                                            .friendships
                                            .iter()
                                            .any(|fr| fr.other_id == other_id)
                                        {
                                            let updated =
                                                db.friendships.remove(user_me.id, other_id).await;
                                            if updated {
                                                db.users.invalidate_cache(&user_me.id);
                                                db.users.invalidate_cache(&other_id);
                                            }
                                            updated
                                        } else {
                                            false
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        false
                    }
                }
                .into_actor(self),
            )
        }
    }

    pub struct BattleReq {
        pub sender_addr: Addr<ClientState>,
        pub sender_uid: UserId,
        pub receiver_uid: UserId,
    }
    impl Message for BattleReq {
        type Result = ();
    }
    impl Handler<BattleReq> for UserManager {
        type Result = ResponseActFuture<Self, ()>;
        fn handle(&mut self, msg: BattleReq, _ctx: &mut Self::Context) -> Self::Result {
            let db = self.db.clone();
            let lobby_mgr = self.lobby_mgr_state.clone();

            Box::pin(
                async move {
                    if let BacklinkState::Linked(lobby_mgr) = &lobby_mgr {
                        if let Some(receiver) =
                            db.users.get_id(&msg.receiver_uid, &db.friendships).await
                        {
                            if let Some(receiver_addr) = &receiver.playing {
                                lobby_mgr.do_send(lobby_mgr::BattleReq {
                                    sender_addr: msg.sender_addr,
                                    sender_uid: msg.sender_uid,
                                    receiver_addr: receiver_addr.clone(),
                                    receiver_uid: msg.receiver_uid,
                                });
                            } else {
                                msg.sender_addr.do_send(ServerMessage::Error(Some(
                                    SrvMsgError::UserNotPlaying,
                                )));
                            }
                        } else {
                            // println!("no such user: {}", msg.receiver);
                            msg.sender_addr
                                .do_send(ServerMessage::Error(Some(SrvMsgError::NoSuchUser)));
                        }
                    } else {
                        msg.sender_addr
                            .do_send(ServerMessage::Error(Some(SrvMsgError::Internal)));
                    }
                }
                .into_actor(self),
            )
        }
    }

    // ── Task 5.4: StartGame ────────────────────────────────────────────────

    pub struct GetStartGameData {
        pub user_id: UserId,
    }
    impl Message for GetStartGameData {
        type Result = Result<GameMsgOut, ()>;
    }
    impl Handler<GetStartGameData> for UserManager {
        type Result = ResponseActFuture<Self, Result<GameMsgOut, ()>>;
        fn handle(&mut self, msg: GetStartGameData, _ctx: &mut Self::Context) -> Self::Result {
            let db = self.db.clone();
            let game_config = self.game_config.clone();
            Box::pin(
                async move {
                    // Task 5.8: lazy backfill
                    if let Err(e) = db.players.ensure_mode_progress(&msg.user_id, &game_config).await {
                        log::warn!("ensure_mode_progress failed for {}: {:?}", msg.user_id, e);
                    }
                    let agg = match db.players.get_readonly(&msg.user_id).await {
                        Ok(a) => a,
                        Err(_) => return Err(()),
                    };
                    let journey_progress = agg.mode_progress.get(&1).copied().unwrap_or(0);

                    let modes: Vec<ModeStatus> = game_config
                        .modes
                        .iter()
                        .map(|m| {
                            let current_level =
                                agg.mode_progress.get(&m.mode).copied().unwrap_or(0);
                            let total = game_config
                                .level_count_by_mode
                                .get(&m.mode)
                                .copied()
                                .unwrap_or(0);
                            // unlock_by_journey_level <= 1 means always available (starter modes);
                            // otherwise require journey_progress >= threshold
                            let unlocked = m.unlock_by_journey_level <= 1
                                || journey_progress >= m.unlock_by_journey_level;
                            ModeStatus {
                                mode: m.mode,
                                unlocked,
                                current_level,
                                total_levels: total as i32,
                            }
                        })
                        .collect();

                    // For each unlocked mode, the current next-level entry
                    let levels: Vec<LevelEntry> = modes
                        .iter()
                        .filter(|m| m.unlocked)
                        .filter_map(|m| {
                            game_config
                                .level_by_id(m.mode, m.current_level + 1)
                                .map(|l| level_entry_from_config(l, 0))
                        })
                        .collect();

                    Ok(GameMsgOut::StartGameResp { modes, levels })
                }
                .into_actor(self),
            )
        }
    }

    // ── Task 5.5: LevelPage ────────────────────────────────────────────────

    pub struct GetLevelPage {
        pub user_id: UserId,
        pub mode: i32,
        pub after_id: i32,
        pub page_size: usize,
    }
    impl Message for GetLevelPage {
        type Result = Result<GameMsgOut, ()>;
    }
    impl Handler<GetLevelPage> for UserManager {
        type Result = ResponseActFuture<Self, Result<GameMsgOut, ()>>;
        fn handle(&mut self, msg: GetLevelPage, _ctx: &mut Self::Context) -> Self::Result {
            let game_config = self.game_config.clone();
            Box::pin(
                async move {
                    let all_levels = game_config.levels_in_mode(msg.mode);
                    let start = all_levels.partition_point(|l| l.id <= msg.after_id);
                    let remaining = &all_levels[start..];
                    let has_more = remaining.len() > msg.page_size;
                    let levels: Vec<LevelEntry> = remaining
                        .iter()
                        .take(msg.page_size)
                        .map(|l| level_entry_from_config(l, 0))
                        .collect();
                    Ok(GameMsgOut::LevelPageResp { levels, has_more })
                }
                .into_actor(self),
            )
        }
    }

    // ── Task 5.6: CompleteLevel ────────────────────────────────────────────

    pub struct SubmitCompleteLevel {
        pub user_id: UserId,
        pub mode: i32,
        pub level_id: i32,
        pub stars: i32,
    }
    impl Message for SubmitCompleteLevel {
        type Result = Result<GameMsgOut, ()>;
    }
    impl Handler<SubmitCompleteLevel> for UserManager {
        type Result = ResponseActFuture<Self, Result<GameMsgOut, ()>>;
        fn handle(&mut self, msg: SubmitCompleteLevel, _ctx: &mut Self::Context) -> Self::Result {
            let db = self.db.clone();
            let game_config = self.game_config.clone();
            let activity_log = self.activity_log.clone();
            let connection_mgr = self.connection_mgr.clone();
            Box::pin(
                async move {
                    // P1.3: validate (mode, level_id) exists in config before any DB work
                    if game_config.level_by_id(msg.mode, msg.level_id).is_none() {
                        return Ok(GameMsgOut::CompleteLevelResp { ok: false, new_level: 0, rewards: vec![] });
                    }

                    // Task 5.8: lazy backfill before access
                    if let Err(e) = db.players.ensure_mode_progress(&msg.user_id, &game_config).await {
                        log::warn!("ensure_mode_progress failed for {}: {:?}", msg.user_id, e);
                    }

                    // Mode unlock check (business layer, before DB transaction)
                    let agg = match db.players.get_readonly(&msg.user_id).await {
                        Ok(a) => a,
                        Err(_) => return Ok(GameMsgOut::CompleteLevelResp { ok: false, new_level: 0, rewards: vec![] }),
                    };
                    let journey_progress = agg.mode_progress.get(&1).copied().unwrap_or(0);
                    if let Some(mode_cfg) = game_config.mode_config(msg.mode) {
                        // unlock_by_journey_level <= 1 = always available; else need journey_progress >= threshold
                        if mode_cfg.unlock_by_journey_level > 1
                            && journey_progress < mode_cfg.unlock_by_journey_level
                        {
                            return Ok(GameMsgOut::CompleteLevelResp { ok: false, new_level: 0, rewards: vec![] });
                        }
                    } else {
                        return Ok(GameMsgOut::CompleteLevelResp { ok: false, new_level: 0, rewards: vec![] });
                    }

                    match db.players.complete_level(&msg.user_id, msg.mode, msg.level_id, msg.stars).await {
                        Ok(outcome) => {
                            activity_log.record(ActivityEvent::new(
                                Some(msg.user_id.to_string()),
                                ActivityEventKind::CompleteLevel,
                                Some(serde_json::json!({
                                    "mode": msg.mode,
                                    "level_id": msg.level_id,
                                    "stars": msg.stars
                                })),
                            ));
                            // Push achievement unlocks
                            for (achievement_id, tier) in &outcome.achievements {
                                connection_mgr.do_send(PushToUser {
                                    user_id: msg.user_id,
                                    msg: GameMsgOut::AchievementUnlocked {
                                        achievement_id: achievement_id.clone(),
                                        tier: *tier,
                                    },
                                });
                            }
                            // Progressive milestones → inbox badge + WS push
                            if !outcome.milestones.is_empty() {
                                set_badge(&db.pool, &msg.user_id, MODULE_INBOX).await;
                                for (achievement_id, step, inbox_id) in &outcome.milestones {
                                    connection_mgr.do_send(PushToUser {
                                        user_id: msg.user_id,
                                        msg: GameMsgOut::ProgressiveMilestone {
                                            achievement_id: achievement_id.clone(),
                                            step: *step,
                                        },
                                    });
                                    connection_mgr.do_send(PushToUser {
                                        user_id: msg.user_id,
                                        msg: GameMsgOut::NewInboxMessage {
                                            id: *inbox_id,
                                            msg_type: "progressive_achievement".to_string(),
                                            has_reward: true,
                                        },
                                    });
                                }
                            }
                            // Quest completions → quests badge + WS push
                            if !outcome.completed_quests.is_empty() {
                                set_badge(&db.pool, &msg.user_id, MODULE_QUESTS).await;
                                for (quest_id, quest_type) in &outcome.completed_quests {
                                    connection_mgr.do_send(PushToUser {
                                        user_id: msg.user_id,
                                        msg: GameMsgOut::QuestCompleted {
                                            quest_id: quest_id.clone(),
                                            quest_type: quest_type.clone(),
                                        },
                                    });
                                }
                            }
                            let rewards = outcome.rewards.iter()
                                .map(|(item_id, amount)| RewardEntry { item_id: item_id.clone(), amount: *amount })
                                .collect();
                            Ok(GameMsgOut::CompleteLevelResp { ok: true, new_level: outcome.new_level, rewards })
                        }
                        Err(e) => {
                            log::warn!(
                                "complete_level rejected user={} mode={} level_id={}: {:?}",
                                msg.user_id, msg.mode, msg.level_id, e
                            );
                            Ok(GameMsgOut::CompleteLevelResp { ok: false, new_level: 0, rewards: vec![] })
                        }
                    }
                }
                .into_actor(self),
            )
        }
    }
}

fn level_entry_from_config(l: &crate::config::LevelConfig, best_stars: i32) -> LevelEntry {
    LevelEntry {
        id: l.id,
        mode: l.mode,
        best_stars,
        level_min: l.level_min,
        level_max: l.level_max,
        chapter: l.chapter,
        ammo_count: l.ammo_count,
        triple_star: l.triple_star,
        double_star: l.double_star,
        single_star: l.single_star,
        scene_id: l.scene_id,
    }
}
