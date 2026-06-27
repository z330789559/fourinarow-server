use actix::prelude::*;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

const BATCH_SIZE: usize = 100;
const FLUSH_INTERVAL_SECS: u64 = 5;
const MAILBOX_CAPACITY: usize = 8192;
const WARN_EVERY_DROPS: u64 = 100;

// ---------------------------------------------------------------------------
// Public event types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityEventKind {
    Register,
    Login,
    Logout,
    Offline,
    Purchase,
    UseItem,
    CompleteQuest,
    ClaimReward,
    AddFriend,
    CompleteLevel,
}

impl ActivityEventKind {
    pub fn as_str(&self) -> &'static str {
        use ActivityEventKind::*;
        match self {
            Register => "register",
            Login => "login",
            Logout => "logout",
            Offline => "offline",
            Purchase => "purchase",
            UseItem => "use_item",
            CompleteQuest => "complete_quest",
            ClaimReward => "claim_reward",
            AddFriend => "add_friend",
            CompleteLevel => "complete_level",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActivityEvent {
    pub user_id: Option<String>,
    pub action_type: String,
    pub detail: Option<Value>,
    pub created_at: DateTime<Utc>,
}

impl ActivityEvent {
    pub fn new(user_id: Option<String>, kind: ActivityEventKind, detail: Option<Value>) -> Self {
        ActivityEvent {
            user_id,
            action_type: kind.as_str().to_string(),
            detail,
            created_at: Utc::now(),
        }
    }
}

impl Message for ActivityEvent {
    type Result = ();
}

// ---------------------------------------------------------------------------
// ForceFlush message (for graceful shutdown)
// ---------------------------------------------------------------------------

pub struct ForceFlush;
impl Message for ForceFlush {
    type Result = ();
}

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

struct ActivityLogger {
    pool: PgPool,
    buffer: Vec<ActivityEvent>,
}

impl ActivityLogger {
    fn new(pool: PgPool) -> Self {
        ActivityLogger {
            pool,
            buffer: Vec::with_capacity(BATCH_SIZE + 16),
        }
    }

    fn flush_buffer(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let events = std::mem::take(&mut self.buffer);
        let pool = self.pool.clone();
        tokio::spawn(async move {
            flush_events_to_db(&pool, events).await;
        });
    }
}

impl Actor for ActivityLogger {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(MAILBOX_CAPACITY);
        ctx.run_interval(
            std::time::Duration::from_secs(FLUSH_INTERVAL_SECS),
            |act, _ctx| {
                act.flush_buffer();
            },
        );
    }
}

impl Handler<ActivityEvent> for ActivityLogger {
    type Result = ();

    fn handle(&mut self, event: ActivityEvent, _ctx: &mut Self::Context) {
        self.buffer.push(event);
        if self.buffer.len() >= BATCH_SIZE {
            self.flush_buffer();
        }
    }
}

impl Handler<ForceFlush> for ActivityLogger {
    type Result = ();

    fn handle(&mut self, _: ForceFlush, _ctx: &mut Self::Context) {
        self.flush_buffer();
    }
}

// ---------------------------------------------------------------------------
// DB flush (batch insert within a transaction)
// ---------------------------------------------------------------------------

async fn flush_events_to_db(pool: &PgPool, events: Vec<ActivityEvent>) {
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            log::warn!("activity_log flush: failed to begin tx: {:?}", err);
            return;
        }
    };

    for event in &events {
        if let Err(err) = sqlx::query(
            "INSERT INTO activity_log (user_id, action_type, detail, created_at) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&event.user_id)
        .bind(&event.action_type)
        .bind(&event.detail)
        .bind(event.created_at)
        .execute(&mut *tx)
        .await
        {
            log::warn!("activity_log flush: insert failed, rolling back batch: {:?}", err);
            let _ = tx.rollback().await;
            return;
        }
    }

    if let Err(err) = tx.commit().await {
        log::warn!("activity_log flush: commit failed: {:?}", err);
    }
}

// ---------------------------------------------------------------------------
// Public handle (the only surface exposed to callers)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ActivityLogHandle {
    addr: Addr<ActivityLogger>,
    drop_counter: Arc<AtomicU64>,
}

impl ActivityLogHandle {
    pub fn start(pool: PgPool) -> Self {
        let addr = ActivityLogger::new(pool).start();
        ActivityLogHandle {
            addr,
            drop_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Non-blocking record. Drops silently when mailbox is full; warns every WARN_EVERY_DROPS drops.
    pub fn record(&self, event: ActivityEvent) {
        if let Err(_) = self.addr.try_send(event) {
            let count = self.drop_counter.fetch_add(1, Ordering::Relaxed) + 1;
            if count % WARN_EVERY_DROPS == 0 {
                log::warn!(
                    "ActivityLogger mailbox full — {} events dropped so far",
                    count
                );
            }
        }
    }

    /// Trigger an immediate flush of the internal buffer (for graceful shutdown).
    pub fn force_flush(&self) {
        self.addr.do_send(ForceFlush);
    }
}
