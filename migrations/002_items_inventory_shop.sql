CREATE TABLE IF NOT EXISTS items (
    id          VARCHAR(50) PRIMARY KEY,
    name        VARCHAR(100) NOT NULL,
    item_type   VARCHAR(20)  NOT NULL DEFAULT 'currency',
    description TEXT         NOT NULL DEFAULT '',
    icon_url    TEXT         NOT NULL DEFAULT '',
    stackable   BOOLEAN      NOT NULL DEFAULT true,
    max_stack   INTEGER      NOT NULL DEFAULT 9999,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

INSERT INTO items (id, name, item_type, description, stackable, max_stack) VALUES
    ('coin',   'Coin',   'currency',   'In-game currency', true, 999999),
    ('gem',    'Gem',    'currency',   'Premium currency', true,  99999),
    ('ticket', 'Ticket', 'consumable', 'Play ticket',      true,    999)
ON CONFLICT DO NOTHING;

CREATE TABLE IF NOT EXISTS user_inventory (
    id          BIGSERIAL   PRIMARY KEY,
    user_id     VARCHAR(12) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    item_id     VARCHAR(50) NOT NULL REFERENCES items(id),
    quantity    INTEGER     NOT NULL DEFAULT 1 CHECK (quantity >= 0),
    acquired_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (user_id, item_id)
);

CREATE INDEX IF NOT EXISTS idx_user_inventory_user ON user_inventory (user_id);

CREATE TABLE IF NOT EXISTS shop_items (
    id            BIGSERIAL   PRIMARY KEY,
    shop_id       VARCHAR(50) NOT NULL DEFAULT 'default',
    item_id       VARCHAR(50) NOT NULL REFERENCES items(id),
    price_item_id VARCHAR(50) NOT NULL REFERENCES items(id),
    price         INTEGER     NOT NULL DEFAULT 0,
    stock         INTEGER,
    refresh_at    TIMESTAMPTZ,
    enabled       BOOLEAN     NOT NULL DEFAULT true,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (shop_id, item_id)
);

CREATE INDEX IF NOT EXISTS idx_shop_items_shop ON shop_items (shop_id);
