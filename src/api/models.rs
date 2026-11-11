use serde::{Deserialize, Serialize};

/// Server timestamps are milliseconds since the Unix epoch.
pub type Timestamp = i64;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Health {
    pub msg: String,
    /// Backend build number; 0.7.10 is 124.
    pub version: u32,
    #[serde(default)]
    pub min_frontend_version: u32,
    #[serde(default)]
    pub disable_username_password_login: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub username: String,
    #[serde(default)]
    pub admin: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Member {
    pub id: i64,
    pub name: String,
    pub username: String,
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub owner: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Household {
    pub id: i64,
    pub name: String,
    pub default_shopping_list: Option<ListInfo>,
    #[serde(default)]
    pub member: Vec<Member>,
}

/// A shopping list without its items, as returned when lists are added or renamed.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ListInfo {
    pub id: i64,
    pub name: String,
    pub household_id: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ShoppingList {
    pub id: i64,
    pub name: String,
    pub household_id: i64,
    #[serde(default)]
    pub items: Vec<ListItem>,
    #[serde(default, rename = "recentItems")]
    pub recent_items: Vec<ListItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Category {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub ordering: i64,
    #[serde(default)]
    pub default: bool,
    pub household_id: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Item {
    pub id: i64,
    pub name: String,
    /// KitchenOwl icon name. Sent back only when the user picks a new icon, so updates don't
    /// clear one set in another client.
    pub icon: Option<String>,
    pub category: Option<Category>,
    /// Seeded starter item.
    #[serde(default)]
    pub default: bool,
    pub default_key: Option<String>,
    /// Learned shopping order; 0 means none yet.
    #[serde(default)]
    pub ordering: i64,
    #[serde(default)]
    pub support: f64,
    pub household_id: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Item {
    /// Starter items count as used once edited or given a shopping order. A heuristic: the
    /// server keeps no direct usage flag.
    pub fn is_used(&self) -> bool {
        !self.default
            || self.created_at != self.updated_at
            || self.ordering > 0
            || self.support > 0.0
    }
}

/// An item as it sits on a list, with that list's description.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ListItem {
    #[serde(flatten)]
    pub item: Item,
    #[serde(default)]
    pub description: String,
    pub created_by: Option<i64>,
}
