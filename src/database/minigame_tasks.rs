//! 英歌任务数据层（设计：darkHero docs/superpowers/specs/2026-06-28-yingge-task-system-design.md）。
//!
//! 任务定义在 `api::minigame_tasks` 静态配置；这里只管用户进度/记账与聚合派生。
//! 沿用项目 runtime sqlx 查询风格（非 query! 宏，cargo check 不依赖在线 DB）。

use chrono::NaiveDate;
use sqlx::PgPool;

pub struct MinigameTaskCollection {
    pool: PgPool,
}

/// 从 minigame_level_score 派生的玩家聚合量（驱动主线/成就）。
#[derive(Debug, Clone, Default)]
pub struct MinigameAggregate {
    pub levels_cleared: i64,
    pub total_stars: i64,
    pub total_score: i64,
}

impl MinigameTaskCollection {
    pub fn new(pool: PgPool) -> Self {
        MinigameTaskCollection { pool }
    }

    /// 通关关数(best_stars>=1) / 累计星 / 累计分。
    pub async fn aggregate(&self, game_key: &str, user_id: &str) -> MinigameAggregate {
        let row: Option<(i64, i64, i64)> = sqlx::query_as(
            "SELECT \
                 COUNT(*) FILTER (WHERE best_stars >= 1)::bigint, \
                 COALESCE(SUM(best_stars), 0)::bigint, \
                 COALESCE(SUM(best_score), 0)::bigint \
             FROM minigame_level_score WHERE game_key = $1 AND user_id = $2",
        )
        .bind(game_key)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        row.map(|(c, st, sc)| MinigameAggregate {
            levels_cleared: c,
            total_stars: st,
            total_score: sc,
        })
        .unwrap_or_default()
    }

    /// 今日 (plays, clears)。
    pub async fn daily(&self, game_key: &str, user_id: &str, day: NaiveDate) -> (i64, i64) {
        let row: Option<(i32, i32)> = sqlx::query_as(
            "SELECT plays, clears FROM minigame_daily_activity \
             WHERE user_id = $1 AND game_key = $2 AND day = $3",
        )
        .bind(user_id)
        .bind(game_key)
        .bind(day)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        row.map(|(p, c)| (p as i64, c as i64)).unwrap_or((0, 0))
    }

    /// 每局结束累加：plays+1，cleared 时 clears+1。fire-and-forget（失败静默）。
    pub async fn bump_daily(&self, game_key: &str, user_id: &str, day: NaiveDate, cleared: bool) {
        let cl: i32 = if cleared { 1 } else { 0 };
        let _ = sqlx::query(
            "INSERT INTO minigame_daily_activity (user_id, game_key, day, plays, clears) \
             VALUES ($1, $2, $3, 1, $4) \
             ON CONFLICT (user_id, game_key, day) DO UPDATE SET \
                 plays = minigame_daily_activity.plays + 1, \
                 clears = minigame_daily_activity.clears + EXCLUDED.clears",
        )
        .bind(user_id)
        .bind(game_key)
        .bind(day)
        .bind(cl)
        .execute(&self.pool)
        .await;
    }

    /// 签到统计 (total, signed_today)。
    pub async fn signin(&self, game_key: &str, user_id: &str, today: NaiveDate) -> (i64, bool) {
        let row: Option<(i32, Option<NaiveDate>)> = sqlx::query_as(
            "SELECT total, last_date FROM minigame_signin_stat \
             WHERE user_id = $1 AND game_key = $2",
        )
        .bind(user_id)
        .bind(game_key)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        match row {
            Some((t, last)) => (t as i64, last == Some(today)),
            None => (0, false),
        }
    }

    /// 上报签到：当日首次则 total+1 且 last_date=today（同日重复无副作用）。
    pub async fn report_signin(&self, game_key: &str, user_id: &str, today: NaiveDate) {
        let _ = sqlx::query(
            "INSERT INTO minigame_signin_stat (user_id, game_key, total, last_date) \
             VALUES ($1, $2, 1, $3) \
             ON CONFLICT (user_id, game_key) DO UPDATE SET \
                 total = minigame_signin_stat.total \
                     + (CASE WHEN minigame_signin_stat.last_date IS DISTINCT FROM EXCLUDED.last_date THEN 1 ELSE 0 END), \
                 last_date = EXCLUDED.last_date",
        )
        .bind(user_id)
        .bind(game_key)
        .bind(today)
        .execute(&self.pool)
        .await;
    }

    pub async fn is_claimed(
        &self,
        game_key: &str,
        user_id: &str,
        task_id: &str,
        day: NaiveDate,
    ) -> bool {
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT 1 FROM minigame_task_claim \
             WHERE user_id = $1 AND game_key = $2 AND task_id = $3 AND day = $4",
        )
        .bind(user_id)
        .bind(game_key)
        .bind(task_id)
        .bind(day)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        row.is_some()
    }

    /// 记账领取；返回 true=本次插入(成功)，false=已存在(重复领)。原子防重复领。
    pub async fn record_claim(
        &self,
        game_key: &str,
        user_id: &str,
        task_id: &str,
        day: NaiveDate,
    ) -> bool {
        match sqlx::query(
            "INSERT INTO minigame_task_claim (user_id, game_key, task_id, day) \
             VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(game_key)
        .bind(task_id)
        .bind(day)
        .execute(&self.pool)
        .await
        {
            Ok(r) => r.rows_affected() > 0,
            Err(_) => false,
        }
    }

    pub async fn get_step(&self, game_key: &str, user_id: &str, achievement_id: &str) -> i64 {
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT step FROM minigame_task_achievement_progress \
             WHERE user_id = $1 AND game_key = $2 AND achievement_id = $3",
        )
        .bind(user_id)
        .bind(game_key)
        .bind(achievement_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        row.map(|(s,)| s as i64).unwrap_or(0)
    }

    /// 推进成就轮次：仅当当前 step == expected_step 时 +1（乐观锁防并发/重复领）。返回 true=推进成功。
    /// expected_step=0 ⟺ 无进度行（首领），走 INSERT 分支插入 step=1。
    pub async fn advance_step(
        &self,
        game_key: &str,
        user_id: &str,
        achievement_id: &str,
        expected_step: i64,
    ) -> bool {
        match sqlx::query(
            "INSERT INTO minigame_task_achievement_progress (user_id, game_key, achievement_id, step) \
             VALUES ($1, $2, $3, 1) \
             ON CONFLICT (user_id, game_key, achievement_id) DO UPDATE SET \
                 step = minigame_task_achievement_progress.step + 1 \
             WHERE minigame_task_achievement_progress.step = $4",
        )
        .bind(user_id)
        .bind(game_key)
        .bind(achievement_id)
        .bind(expected_step as i32)
        .execute(&self.pool)
        .await
        {
            Ok(r) => r.rows_affected() > 0,
            Err(_) => false,
        }
    }
}
