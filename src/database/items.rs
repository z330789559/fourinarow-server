use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::api::users::user::UserId;

pub struct ItemCollection {
    pool: PgPool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InventoryEntry {
    pub item_id: String,
    pub quantity: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShopItem {
    pub shop_id: String,
    pub item_id: String,
    pub price_item_id: String,
    pub price: i32,
    pub stock: Option<i32>,
}

#[derive(Debug)]
pub enum ItemError {
    NotEnoughItems,
    ItemNotFound,
    DbError,
}

impl ItemCollection {
    pub fn new(pool: PgPool) -> Self {
        ItemCollection { pool }
    }

    pub async fn get_inventory(&self, user_id: &UserId) -> Vec<InventoryEntry> {
        sqlx::query_as::<_, InventoryEntry>(
            "SELECT item_id, quantity FROM user_inventory WHERE user_id = $1 AND quantity > 0",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
    }

    async fn get_balance(&self, user_id: &UserId, item_id: &str) -> i32 {
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT quantity FROM user_inventory WHERE user_id = $1 AND item_id = $2",
        )
        .bind(user_id.to_string())
        .bind(item_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        row.map(|(quantity,)| quantity).unwrap_or(0)
    }

    async fn add_item(&self, user_id: &UserId, item_id: &str, qty: i32) -> Result<(), ItemError> {
        sqlx::query(
            "INSERT INTO user_inventory (user_id, item_id, quantity) VALUES ($1, $2, $3) \
             ON CONFLICT (user_id, item_id) DO UPDATE SET quantity = user_inventory.quantity + $3",
        )
        .bind(user_id.to_string())
        .bind(item_id)
        .bind(qty)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|_| ItemError::DbError)
    }

    async fn consume_item(
        &self,
        user_id: &UserId,
        item_id: &str,
        qty: i32,
    ) -> Result<(), ItemError> {
        let rows_affected = sqlx::query(
            "UPDATE user_inventory SET quantity = quantity - $3 \
             WHERE user_id = $1 AND item_id = $2 AND quantity >= $3",
        )
        .bind(user_id.to_string())
        .bind(item_id)
        .bind(qty)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected())
        .map_err(|_| ItemError::DbError)?;

        if rows_affected == 0 {
            Err(ItemError::NotEnoughItems)
        } else {
            Ok(())
        }
    }

    async fn purchase(
        &self,
        user_id: &UserId,
        shop_id: &str,
        item_id: &str,
    ) -> Result<(), ItemError> {
        let row: Option<(String, i32, Option<i32>)> = sqlx::query_as(
            "SELECT price_item_id, price, stock FROM shop_items \
             WHERE shop_id = $1 AND item_id = $2 AND enabled = true",
        )
        .bind(shop_id)
        .bind(item_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let (price_item_id, price, stock) = row.ok_or(ItemError::ItemNotFound)?;
        if let Some(stock_left) = stock {
            if stock_left <= 0 {
                return Err(ItemError::NotEnoughItems);
            }
        }

        self.consume_item(user_id, &price_item_id, price).await?;
        self.add_item(user_id, item_id, 1).await?;

        if stock.is_some() {
            let _ = sqlx::query(
                "UPDATE shop_items SET stock = stock - 1 WHERE shop_id = $1 AND item_id = $2",
            )
            .bind(shop_id)
            .bind(item_id)
            .execute(&self.pool)
            .await;
        }

        Ok(())
    }

    pub async fn get_shop(&self, shop_id: &str) -> Vec<ShopItem> {
        let rows: Vec<(String, String, String, i32, Option<i32>)> = sqlx::query_as(
            "SELECT shop_id, item_id, price_item_id, price, stock FROM shop_items \
             WHERE shop_id = $1 AND enabled = true",
        )
        .bind(shop_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .map(|(shop_id, item_id, price_item_id, price, stock)| ShopItem {
                shop_id,
                item_id,
                price_item_id,
                price,
                stock,
            })
            .collect()
    }
}
