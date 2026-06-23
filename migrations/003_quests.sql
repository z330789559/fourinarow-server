CREATE TABLE IF NOT EXISTS quests (
    id              VARCHAR(50)  PRIMARY KEY,
    quest_type      VARCHAR(20)  NOT NULL DEFAULT 'story',
    title           VARCHAR(100) NOT NULL,
    description     TEXT         NOT NULL DEFAULT '',
    condition_type  VARCHAR(50)  NOT NULL,
    condition_value INTEGER      NOT NULL DEFAULT 1,
    reward_item_id  VARCHAR(50)  REFERENCES items(id),
    reward_quantity INTEGER      NOT NULL DEFAULT 0,
    sort_order      INTEGER      NOT NULL DEFAULT 0,
    next_quest_id   VARCHAR(50)  REFERENCES quests(id),
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

INSERT INTO quests (id, quest_type, title, description, condition_type, condition_value, reward_item_id, reward_quantity, sort_order, next_quest_id) VALUES
    ('story_first_win',  'story', 'First Victory',   'Win your first game',   'games_won',    1,  'coin', 50,  1, 'story_ten_wins'),
    ('story_ten_wins',   'story', 'Ten Victories',   'Win 10 games',          'games_won',    10, 'coin', 200, 2, NULL),
    ('daily_play_1',     'daily', 'Daily Play',      'Play 1 game today',     'games_played', 1,  'coin', 20,  1, NULL),
    ('daily_win_1',      'daily', 'Daily Win',       'Win 1 game today',      'games_won',    1,  'coin', 30,  2, NULL)
ON CONFLICT DO NOTHING;

CREATE TABLE IF NOT EXISTS user_quest_progress (
    id            BIGSERIAL   PRIMARY KEY,
    user_id       VARCHAR(12) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    quest_id      VARCHAR(50) NOT NULL REFERENCES quests(id),
    current_value INTEGER     NOT NULL DEFAULT 0,
    completed_at  TIMESTAMPTZ,
    rewarded      BOOLEAN     NOT NULL DEFAULT false,
    quest_date    DATE
);

CREATE INDEX IF NOT EXISTS idx_uqp_user ON user_quest_progress (user_id);
CREATE UNIQUE INDEX IF NOT EXISTS uq_user_quest_progress
    ON user_quest_progress (user_id, quest_id, COALESCE(quest_date, '1970-01-01'::date));

CREATE TABLE IF NOT EXISTS achievement_tiers (
    id               BIGSERIAL    PRIMARY KEY,
    achievement_id   VARCHAR(50)  NOT NULL,
    tier             INTEGER      NOT NULL DEFAULT 1,
    title            VARCHAR(100) NOT NULL DEFAULT '',
    condition_value  INTEGER      NOT NULL DEFAULT 1,
    reward_item_id   VARCHAR(50)  REFERENCES items(id),
    reward_quantity  INTEGER      NOT NULL DEFAULT 0,
    UNIQUE (achievement_id, tier)
);

INSERT INTO achievement_tiers (achievement_id, tier, title, condition_value, reward_item_id, reward_quantity) VALUES
    ('wins_milestone', 1, 'Win 10 games',   10,  'coin', 100),
    ('wins_milestone', 2, 'Win 100 games',  100, 'coin', 500),
    ('wins_milestone', 3, 'Win 1000 games', 1000,'gem',  10)
ON CONFLICT DO NOTHING;

CREATE TABLE IF NOT EXISTS user_achievement_progress (
    id             BIGSERIAL   PRIMARY KEY,
    user_id        VARCHAR(12) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    achievement_id VARCHAR(50) NOT NULL,
    current_tier   INTEGER     NOT NULL DEFAULT 1,
    current_value  INTEGER     NOT NULL DEFAULT 0,
    UNIQUE (user_id, achievement_id)
);

CREATE INDEX IF NOT EXISTS idx_uap_user ON user_achievement_progress (user_id);
