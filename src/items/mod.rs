use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ItemType {
    Currency,
    Consumable,
    Cosmetic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDef {
    pub id: &'static str,
    pub name: &'static str,
    pub item_type: ItemType,
    pub description: &'static str,
    pub stackable: bool,
    pub max_stack: i32,
}

pub const ITEMS: &[ItemDef] = &[
    ItemDef {
        id: "coin",
        name: "Coin",
        item_type: ItemType::Currency,
        description: "In-game currency",
        stackable: true,
        max_stack: 999999,
    },
    ItemDef {
        id: "gem",
        name: "Gem",
        item_type: ItemType::Currency,
        description: "Premium currency",
        stackable: true,
        max_stack: 99999,
    },
    ItemDef {
        id: "ticket",
        name: "Ticket",
        item_type: ItemType::Consumable,
        description: "Play ticket",
        stackable: true,
        max_stack: 999,
    },
];

pub fn find_item(id: &str) -> Option<&'static ItemDef> {
    ITEMS.iter().find(|item| item.id == id)
}
