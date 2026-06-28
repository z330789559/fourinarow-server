//! 英歌小游戏任务 REST 接口（设计：darkHero docs/superpowers/specs/2026-06-28-yingge-task-system-design.md）。
//!
//! GET  /api/minigame/{game_key}/tasks                     (SessionToken) 三类任务 + 进度
//! POST /api/minigame/{game_key}/tasks/{task_id}/claim     (SessionToken) 领取（一次性/轮次推进）
//! POST /api/minigame/{game_key}/tasks/event/signin        (SessionToken) 签到上报
//!
//! 任务定义在本文件静态配置；进度由 minigame_level_score 聚合 + 每日计数 + 签到统计派生。
//! 奖励(gold/energy)由客户端在领取成功后本地落地；服务端只做完成判定 + 一次性/轮次记账（防重复领）。

use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use chrono::{NaiveDate, Utc};
use serde_json::{json, Value};

use crate::api::get_session_token;
use crate::database::minigame_tasks::MinigameAggregate;
use crate::database::DatabaseManager;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/{game_key}/tasks", web::get().to(get_tasks))
        .route(
            "/{game_key}/tasks/event/signin",
            web::post().to(report_signin),
        )
        .route(
            "/{game_key}/tasks/{task_id}/claim",
            web::post().to(claim_task),
        );
}

/// game_key 校验，沿用 minigame-leaderboard 的约定。
fn validate_game_key(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 64
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 非每日任务领取记账用的哨兵日期。
fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()
}

async fn resolve_user_id(req: &HttpRequest, db: &DatabaseManager) -> Option<String> {
    let token = get_session_token(req)?;
    let user = db.users.get_session_token(token, &db.friendships).await?;
    Some(user.id.to_string())
}

// ——— 奖励 ———

#[derive(Clone, Copy)]
enum RewardKind {
    Gold,
    Energy,
}

impl RewardKind {
    fn as_str(self) -> &'static str {
        match self {
            RewardKind::Gold => "gold",
            RewardKind::Energy => "energy",
        }
    }
}

#[derive(Clone)]
struct Reward {
    kind: RewardKind,
    amount: i64,
}

fn gold(amount: i64) -> Reward {
    Reward {
        kind: RewardKind::Gold,
        amount,
    }
}

fn energy(amount: i64) -> Reward {
    Reward {
        kind: RewardKind::Energy,
        amount,
    }
}

fn rewards_json(rewards: &[Reward]) -> Vec<Value> {
    rewards
        .iter()
        .map(|r| json!({ "type": r.kind.as_str(), "amount": r.amount }))
        .collect()
}

// ——— 进度指标 ———

#[derive(Clone, Copy, PartialEq)]
enum Metric {
    LevelsCleared,
    TotalStars,
    TotalScore,
    Rank,
    DailyPlays,
    DailyClears,
    SigninTotal,
    SignedToday,
}

/// 一次性载入玩家全部派生量，供本次请求所有任务读取。
struct Ctx {
    agg: MinigameAggregate,
    plays: i64,
    clears: i64,
    signin_total: i64,
    signed_today: bool,
    rank: Option<i64>,
}

impl Ctx {
    /// 用于"是否达成"的比较值；Rank 无成绩 → i64::MAX（不满足任何档）。
    fn value(&self, m: Metric) -> i64 {
        match m {
            Metric::LevelsCleared => self.agg.levels_cleared,
            Metric::TotalStars => self.agg.total_stars,
            Metric::TotalScore => self.agg.total_score,
            Metric::Rank => self.rank.unwrap_or(i64::MAX),
            Metric::DailyPlays => self.plays,
            Metric::DailyClears => self.clears,
            Metric::SigninTotal => self.signin_total,
            Metric::SignedToday => {
                if self.signed_today {
                    1
                } else {
                    0
                }
            }
        }
    }

    /// 用于展示的 current（Rank 无成绩 → 0，避免巨数）。
    fn display(&self, m: Metric) -> i64 {
        match m {
            Metric::Rank => self.rank.unwrap_or(0),
            _ => self.value(m),
        }
    }
}

async fn load_ctx(db: &DatabaseManager, game_key: &str, user_id: &str, today: NaiveDate) -> Ctx {
    let agg = db.minigame_tasks.aggregate(game_key, user_id).await;
    let (plays, clears) = db.minigame_tasks.daily(game_key, user_id, today).await;
    let (signin_total, signed_today) = db.minigame_tasks.signin(game_key, user_id, today).await;
    let rank = db
        .minigame_leaderboard
        .get_user_rank(game_key, user_id)
        .await
        .map(|m| m.rank);
    Ctx {
        agg,
        plays,
        clears,
        signin_total,
        signed_today,
        rank,
    }
}

// ——— 简单任务（主线/每日，离散一次性）———

struct SimpleDef {
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    metric: Metric,
    target: i64,
    rewards: Vec<Reward>,
    daily: bool,
}

fn main_defs() -> Vec<SimpleDef> {
    vec![
        SimpleDef { id: "story_clear_1", title: "初阵告捷", desc: "通关 1 关", metric: Metric::LevelsCleared, target: 1, rewards: vec![gold(200)], daily: false },
        SimpleDef { id: "story_clear_5", title: "渐入佳境", desc: "累计通关 5 关", metric: Metric::LevelsCleared, target: 5, rewards: vec![gold(500)], daily: false },
        SimpleDef { id: "story_clear_10", title: "破阵高手", desc: "累计通关 10 关", metric: Metric::LevelsCleared, target: 10, rewards: vec![gold(1000), energy(5)], daily: false },
        SimpleDef { id: "story_stars_15", title: "星光熠熠", desc: "累计获得 15★", metric: Metric::TotalStars, target: 15, rewards: vec![gold(1500)], daily: false },
        SimpleDef { id: "story_clear_33", title: "破阵宗师", desc: "累计通关 33 关", metric: Metric::LevelsCleared, target: 33, rewards: vec![gold(3000), energy(10)], daily: false },
        SimpleDef { id: "story_clear_100", title: "一代宗师", desc: "累计通关 100 关", metric: Metric::LevelsCleared, target: 100, rewards: vec![gold(10000), energy(20)], daily: false },
        SimpleDef { id: "story_signin_1", title: "每日参拜", desc: "累计签到 1 次", metric: Metric::SigninTotal, target: 1, rewards: vec![gold(300), energy(3)], daily: false },
    ]
}

fn daily_defs() -> Vec<SimpleDef> {
    vec![
        SimpleDef { id: "daily_play_1", title: "每日一战", desc: "今日游玩 1 局", metric: Metric::DailyPlays, target: 1, rewards: vec![gold(100)], daily: true },
        SimpleDef { id: "daily_clear_3", title: "三连破阵", desc: "今日通关 3 关", metric: Metric::DailyClears, target: 3, rewards: vec![gold(300), energy(2)], daily: true },
        SimpleDef { id: "daily_play_5", title: "勤勉驱鬼", desc: "今日游玩 5 局", metric: Metric::DailyPlays, target: 5, rewards: vec![energy(5)], daily: true },
        SimpleDef { id: "daily_signin", title: "每日签到", desc: "今日已签到", metric: Metric::SignedToday, target: 1, rewards: vec![gold(200), energy(2)], daily: true },
    ]
}

async fn build_simple(
    db: &DatabaseManager,
    game_key: &str,
    user_id: &str,
    def: &SimpleDef,
    ctx: &Ctx,
    today: NaiveDate,
) -> Value {
    let day = if def.daily { today } else { epoch() };
    let reached = ctx.value(def.metric) >= def.target;
    let claimed = db
        .minigame_tasks
        .is_claimed(game_key, user_id, def.id, day)
        .await;
    let status = if claimed {
        "claimed"
    } else if reached {
        "completed"
    } else {
        "in_progress"
    };
    json!({
        "task_id": def.id,
        "category": if def.daily { "daily" } else { "main" },
        "title": def.title,
        "desc": def.desc,
        "target": def.target,
        "current": ctx.display(def.metric),
        "status": status,
        "rewards": rewards_json(&def.rewards),
    })
}

// ——— 渐进成就（重复轮：达成→领取→下一轮→封顶则 done_all）———

#[derive(Clone, Copy, PartialEq)]
enum Direction {
    Asc,  // current >= threshold（分数/星/通关）
    Desc, // current <= threshold（名次，越小越好）
}

struct Tier {
    threshold: i64,
    desc: String,
    rewards: Vec<Reward>,
}

enum AchKind {
    /// 均匀档：阈值 (init+step)*gap；奖励 gold=(step+1)*gold_coef；cap=None 表示无上限。
    Uniform {
        init: i64,
        gap: i64,
        cap: Option<i64>,
        gold_coef: i64,
    },
    /// 显式档位（非均匀，如排行榜）；cap = 档数。
    Tiers(Vec<Tier>),
}

struct AchDef {
    id: &'static str,
    title: &'static str,
    metric: Metric,
    direction: Direction,
    kind: AchKind,
}

impl AchDef {
    fn cap(&self) -> Option<i64> {
        match &self.kind {
            AchKind::Uniform { cap, .. } => *cap,
            AchKind::Tiers(tiers) => Some(tiers.len() as i64),
        }
    }

    fn threshold(&self, step: i64) -> i64 {
        match &self.kind {
            AchKind::Uniform { init, gap, .. } => (init + step) * gap,
            AchKind::Tiers(tiers) => tiers
                .get(step as usize)
                .map(|t| t.threshold)
                .unwrap_or(i64::MAX),
        }
    }

    fn rewards(&self, step: i64) -> Vec<Reward> {
        match &self.kind {
            AchKind::Uniform { gold_coef, .. } => vec![gold((step + 1) * gold_coef)],
            AchKind::Tiers(tiers) => tiers
                .get(step as usize)
                .map(|t| t.rewards.clone())
                .unwrap_or_default(),
        }
    }

    fn desc(&self, step: i64) -> String {
        match &self.kind {
            AchKind::Uniform { .. } => uniform_desc(self.metric, self.threshold(step)),
            AchKind::Tiers(tiers) => tiers
                .get(step as usize)
                .map(|t| t.desc.clone())
                .unwrap_or_default(),
        }
    }

    fn satisfied(&self, value: i64, step: i64) -> bool {
        let threshold = self.threshold(step);
        match self.direction {
            Direction::Asc => value >= threshold,
            Direction::Desc => value <= threshold,
        }
    }
}

fn uniform_desc(metric: Metric, threshold: i64) -> String {
    match metric {
        Metric::TotalScore => format!("累计总分达到 {}", threshold),
        Metric::TotalStars => format!("累计获得 {}★", threshold),
        Metric::LevelsCleared => format!("累计通关 {} 关", threshold),
        _ => format!("达到 {}", threshold),
    }
}

fn ach_defs() -> Vec<AchDef> {
    vec![
        AchDef {
            id: "rank_board",
            title: "驱鬼榜",
            metric: Metric::Rank,
            direction: Direction::Desc,
            kind: AchKind::Tiers(vec![
                Tier { threshold: 100, desc: "排行榜进入前 100 名".into(), rewards: vec![gold(1000)] },
                Tier { threshold: 50, desc: "排行榜进入前 50 名".into(), rewards: vec![gold(2000), energy(5)] },
                Tier { threshold: 10, desc: "排行榜进入前 10 名".into(), rewards: vec![gold(5000), energy(10)] },
                Tier { threshold: 1, desc: "排行榜登顶第 1 名".into(), rewards: vec![gold(10000), energy(20)] },
            ]),
        },
        AchDef {
            id: "score_master",
            title: "分数大师",
            metric: Metric::TotalScore,
            direction: Direction::Asc,
            kind: AchKind::Uniform { init: 1, gap: 50000, cap: None, gold_coef: 500 },
        },
        AchDef {
            id: "perfectionist",
            title: "完美主义",
            metric: Metric::TotalStars,
            direction: Direction::Asc,
            kind: AchKind::Uniform { init: 1, gap: 10, cap: Some(30), gold_coef: 300 },
        },
    ]
}

fn build_ach(def: &AchDef, ctx: &Ctx, step: i64) -> Value {
    let cap = def.cap();
    let max_round = match cap {
        Some(c) => json!(c),
        None => Value::Null,
    };
    if let Some(c) = cap {
        if step >= c {
            return json!({
                "task_id": def.id,
                "category": "achievement",
                "title": def.title,
                "desc": "已达成全部轮次",
                "target": 0,
                "current": 0,
                "status": "done_all",
                "rewards": [],
                "round": c,
                "max_round": max_round,
            });
        }
    }
    let satisfied = def.satisfied(ctx.value(def.metric), step);
    json!({
        "task_id": def.id,
        "category": "achievement",
        "title": def.title,
        "desc": def.desc(step),
        "target": def.threshold(step),
        "current": ctx.display(def.metric),
        "status": if satisfied { "completed" } else { "in_progress" },
        "rewards": rewards_json(&def.rewards(step)),
        "round": step + 1,
        "max_round": max_round,
    })
}

// ——— Handlers ———

async fn get_tasks(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<String>,
) -> HttpResponse {
    let game_key = path.into_inner();
    if !validate_game_key(&game_key) {
        return HttpResponse::BadRequest().body("invalid game_key");
    }
    let Some(user_id) = resolve_user_id(&req, &db).await else {
        return HttpResponse::Unauthorized().finish();
    };
    let today = Utc::now().date_naive();
    let ctx = load_ctx(&db, &game_key, &user_id, today).await;

    let mut main = Vec::new();
    for def in main_defs() {
        main.push(build_simple(&db, &game_key, &user_id, &def, &ctx, today).await);
    }
    let mut daily = Vec::new();
    for def in daily_defs() {
        daily.push(build_simple(&db, &game_key, &user_id, &def, &ctx, today).await);
    }
    let mut achievement = Vec::new();
    for def in ach_defs() {
        let step = db
            .minigame_tasks
            .get_step(&game_key, &user_id, def.id)
            .await;
        achievement.push(build_ach(&def, &ctx, step));
    }

    HttpResponse::Ok().json(json!({
        "main": main,
        "achievement": achievement,
        "daily": daily,
    }))
}

async fn claim_task(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<(String, String)>,
) -> HttpResponse {
    let (game_key, task_id) = path.into_inner();
    if !validate_game_key(&game_key) {
        return HttpResponse::BadRequest().body("invalid game_key");
    }
    let Some(user_id) = resolve_user_id(&req, &db).await else {
        return HttpResponse::Unauthorized().finish();
    };
    let today = Utc::now().date_naive();
    let ctx = load_ctx(&db, &game_key, &user_id, today).await;

    // 简单任务（主线/每日）：完成判定 + 一次性记账。
    if let Some(def) = main_defs()
        .into_iter()
        .chain(daily_defs())
        .find(|d| task_id == d.id)
    {
        if ctx.value(def.metric) < def.target {
            return HttpResponse::BadRequest().body("task not completed");
        }
        let day = if def.daily { today } else { epoch() };
        if !db
            .minigame_tasks
            .record_claim(&game_key, &user_id, def.id, day)
            .await
        {
            return HttpResponse::Conflict().body("already claimed");
        }
        return HttpResponse::Ok().json(json!({
            "task_id": def.id,
            "rewards": rewards_json(&def.rewards),
            "next": Value::Null,
        }));
    }

    // 渐进成就：完成判定 + 乐观推进 step。
    if let Some(def) = ach_defs().into_iter().find(|d| task_id == d.id) {
        let step = db
            .minigame_tasks
            .get_step(&game_key, &user_id, def.id)
            .await;
        if let Some(c) = def.cap() {
            if step >= c {
                return HttpResponse::Conflict().body("all rounds completed");
            }
        }
        if !def.satisfied(ctx.value(def.metric), step) {
            return HttpResponse::BadRequest().body("round not completed");
        }
        if !db
            .minigame_tasks
            .advance_step(&game_key, &user_id, def.id, step)
            .await
        {
            return HttpResponse::Conflict().body("already claimed");
        }
        let rewards = def.rewards(step);
        let next_step = step + 1;
        let next = if def.cap().map_or(false, |c| next_step >= c) {
            Value::Null
        } else {
            build_ach(&def, &ctx, next_step)
        };
        return HttpResponse::Ok().json(json!({
            "task_id": def.id,
            "rewards": rewards_json(&rewards),
            "next": next,
        }));
    }

    HttpResponse::NotFound().body("task not found")
}

async fn report_signin(
    req: HttpRequest,
    db: web::Data<Arc<DatabaseManager>>,
    path: web::Path<String>,
) -> HttpResponse {
    let game_key = path.into_inner();
    if !validate_game_key(&game_key) {
        return HttpResponse::BadRequest().body("invalid game_key");
    }
    let Some(user_id) = resolve_user_id(&req, &db).await else {
        return HttpResponse::Unauthorized().finish();
    };
    let today = Utc::now().date_naive();
    db.minigame_tasks
        .report_signin(&game_key, &user_id, today)
        .await;
    HttpResponse::Ok().json(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_ach(id: &str) -> AchDef {
        ach_defs().into_iter().find(|d| d.id == id).unwrap()
    }

    #[test]
    fn minigame_tasks_score_master_unbounded() {
        let d = find_ach("score_master");
        assert_eq!(d.cap(), None);
        // 阈值 (init+step)*gap：第1轮 5万、第2轮 10万
        assert_eq!(d.threshold(0), 50000);
        assert_eq!(d.threshold(1), 100000);
        // 奖励 (step+1)*coef：500 / 1000，递增
        assert_eq!(d.rewards(0)[0].amount, 500);
        assert_eq!(d.rewards(1)[0].amount, 1000);
        // 升序达成：>= 门槛
        assert!(d.satisfied(50000, 0));
        assert!(!d.satisfied(49999, 0));
    }

    #[test]
    fn minigame_tasks_perfectionist_capped() {
        let d = find_ach("perfectionist");
        assert_eq!(d.cap(), Some(30));
        assert_eq!(d.threshold(0), 10); // 第1轮 10★
        assert_eq!(d.threshold(29), 300); // 第30轮 300★（满）
        assert_eq!(d.rewards(0)[0].amount, 300);
        assert_eq!(d.rewards(29)[0].amount, 9000);
    }

    #[test]
    fn minigame_tasks_rank_board_desc_tiers() {
        let d = find_ach("rank_board");
        assert_eq!(d.cap(), Some(4));
        assert_eq!(d.threshold(0), 100); // 前100
        assert_eq!(d.threshold(3), 1); // 第1
        // 降序达成：名次 <= 门槛
        assert!(d.satisfied(50, 0)); // 第50名满足"前100"
        assert!(!d.satisfied(150, 0)); // 第150名不满足
        assert!(d.satisfied(1, 3)); // 第1名满足"第1"
        // 显式档位奖励（前50档：金币2000+体力5）
        let r = d.rewards(1);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].kind.as_str(), "gold");
        assert_eq!(r[0].amount, 2000);
        assert_eq!(r[1].kind.as_str(), "energy");
        assert_eq!(r[1].amount, 5);
    }

    #[test]
    fn minigame_tasks_catalog_shape() {
        assert_eq!(main_defs().len(), 7);
        assert_eq!(daily_defs().len(), 4);
        assert_eq!(ach_defs().len(), 3);
    }
}
