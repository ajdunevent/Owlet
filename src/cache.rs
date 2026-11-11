use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::api::{Category, Household, Item, ListItem, ShoppingList, Timestamp, User};

/// Bumped when the stored snapshot shape changes; older snapshots are dropped, queued
/// changes are kept.
const SCHEMA_VERSION: i64 = 1;

/// Everything Owlet shows for the open household. Queued changes are already applied.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub user: Option<User>,
    pub households: Vec<Household>,
    pub lists: Vec<ShoppingList>,
    pub items: Vec<Item>,
    pub categories: Vec<Category>,
    pub synced_at: Timestamp,
    /// Last successful exchange of any kind with the server, live updates included.
    #[serde(default)]
    pub last_contact: Timestamp,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Add {
        list: i64,
        item: i64,
        name: String,
        description: String,
    },
    CheckOff {
        list: i64,
        item: i64,
        at: Timestamp,
    },
}

impl Change {
    pub fn list(&self) -> i64 {
        match self {
            Self::Add { list, .. } | Self::CheckOff { list, .. } => *list,
        }
    }

    pub fn item(&self) -> i64 {
        match self {
            Self::Add { item, .. } | Self::CheckOff { item, .. } => *item,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Queued {
    pub id: i64,
    pub change: Change,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub server: String,
    pub username: String,
}

/// Shows a queued change in `lists` the way the server will once it is sent. Applying a change
/// again has no further effect, so it is safe on a snapshot that already includes it.
pub fn apply(change: &Change, lists: &mut [ShoppingList], items: &[Item], user: Option<i64>) {
    let Some(list) = lists.iter_mut().find(|l| l.id == change.list()) else {
        return;
    };
    match change {
        Change::Add {
            item, description, ..
        } => {
            if list.items.iter().any(|i| i.item.id == *item) {
                return;
            }
            if let Some(found) = items.iter().find(|i| i.id == *item) {
                list.items.push(ListItem {
                    item: found.clone(),
                    description: description.clone(),
                    created_by: user,
                });
            }
        }
        Change::CheckOff { item, .. } => {
            if let Some(position) = list.items.iter().position(|i| i.item.id == *item) {
                let entry = list.items.remove(position);
                list.recent_items.retain(|r| r.item.id != *item);
                list.recent_items.insert(0, entry);
            }
        }
    }
}

pub struct Cache {
    conn: Connection,
}

impl Cache {
    pub fn open_default() -> rusqlite::Result<Self> {
        let dir = glib::user_data_dir().join("owlet");
        if let Err(err) = std::fs::create_dir_all(&dir) {
            log::warn!("Could not create {}: {err}", dir.display());
        }
        Self::open(&dir.join("cache.sqlite"))
    }

    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            conn.execute_batch("DROP TABLE IF EXISTS snapshot")?;
        }
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS snapshot (
                server TEXT NOT NULL, username TEXT NOT NULL, json TEXT NOT NULL,
                PRIMARY KEY (server, username));
             CREATE TABLE IF NOT EXISTS queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                server TEXT NOT NULL, username TEXT NOT NULL, household INTEGER NOT NULL,
                kind TEXT NOT NULL, list INTEGER NOT NULL, item INTEGER NOT NULL,
                name TEXT NOT NULL DEFAULT '', description TEXT NOT NULL DEFAULT '',
                at INTEGER NOT NULL DEFAULT 0);
             PRAGMA user_version = {SCHEMA_VERSION};"
        ))?;
        Ok(Self { conn })
    }

    pub fn save(&self, account: &Account, snapshot: &Snapshot) -> rusqlite::Result<()> {
        let json = serde_json::to_string(snapshot)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO snapshot (server, username, json) VALUES (?1, ?2, ?3)",
            params![account.server, account.username, json],
        )?;
        Ok(())
    }

    /// A snapshot that no longer parses is treated as missing.
    pub fn load(&self, account: &Account) -> Option<Snapshot> {
        let json: String = self
            .conn
            .query_row(
                "SELECT json FROM snapshot WHERE server = ?1 AND username = ?2",
                params![account.server, account.username],
                |r| r.get(0),
            )
            .optional()
            .inspect_err(|err| log::warn!("Could not read the cache: {err}"))
            .ok()??;
        serde_json::from_str(&json)
            .inspect_err(|err| log::warn!("Discarding an unreadable cache entry: {err}"))
            .ok()
    }

    pub fn push(
        &self,
        account: &Account,
        household: i64,
        change: &Change,
    ) -> rusqlite::Result<i64> {
        let (kind, name, description, at) = match change {
            Change::Add {
                name, description, ..
            } => ("add", name.as_str(), description.as_str(), 0),
            Change::CheckOff { at, .. } => ("check-off", "", "", *at),
        };
        self.conn.execute(
            "INSERT INTO queue (server, username, household, kind, list, item, name, description, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                account.server,
                account.username,
                household,
                kind,
                change.list(),
                change.item(),
                name,
                description,
                at
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Queued changes in the order they were made.
    pub fn queued(&self, account: &Account, household: i64) -> Vec<Queued> {
        let result = (|| {
            let mut statement = self.conn.prepare(
                "SELECT id, kind, list, item, name, description, at FROM queue
                 WHERE server = ?1 AND username = ?2 AND household = ?3 ORDER BY id",
            )?;
            let rows =
                statement.query_map(params![account.server, account.username, household], |r| {
                    let kind: String = r.get(1)?;
                    let (list, item) = (r.get(2)?, r.get(3)?);
                    let change = if kind == "add" {
                        Change::Add {
                            list,
                            item,
                            name: r.get(4)?,
                            description: r.get(5)?,
                        }
                    } else {
                        Change::CheckOff {
                            list,
                            item,
                            at: r.get(6)?,
                        }
                    };
                    Ok(Queued {
                        id: r.get(0)?,
                        change,
                    })
                })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })();
        result
            .inspect_err(|err| log::warn!("Could not read queued changes: {err}"))
            .unwrap_or_default()
    }

    pub fn remove(&self, id: i64) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM queue WHERE id = ?1", params![id])
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(username: &str) -> Account {
        Account {
            server: "http://127.0.0.1:5000/".into(),
            username: username.into(),
        }
    }

    #[test]
    fn snapshot_round_trip() {
        let cache = Cache::in_memory().unwrap();
        assert_eq!(cache.load(&account("tester")), None);
        let lists: Vec<ShoppingList> =
            serde_json::from_str(include_str!("api/testdata/shoppinglists.json")).unwrap();
        let snapshot = Snapshot {
            lists,
            synced_at: 1_700_000_000_000,
            ..Default::default()
        };
        cache.save(&account("tester"), &snapshot).unwrap();
        assert_eq!(cache.load(&account("tester")), Some(snapshot));
        assert_eq!(cache.load(&account("other")), None);
    }

    #[test]
    fn queue_keeps_order_and_scope() {
        let cache = Cache::in_memory().unwrap();
        let add = Change::Add {
            list: 1,
            item: 7,
            name: "Milk".into(),
            description: "2 L".into(),
        };
        let check_off = Change::CheckOff {
            list: 1,
            item: 9,
            at: 1_700_000_000_000,
        };
        let me = account("tester");
        let first = cache.push(&me, 1, &add).unwrap();
        cache.push(&me, 1, &check_off).unwrap();
        cache.push(&me, 2, &check_off).unwrap();
        let queued: Vec<Change> = cache.queued(&me, 1).into_iter().map(|q| q.change).collect();
        assert_eq!(queued, vec![add, check_off.clone()]);
        cache.remove(first).unwrap();
        assert_eq!(cache.queued(&me, 1).len(), 1);
        assert_eq!(cache.queued(&me, 2).len(), 1);
        assert!(cache.queued(&account("other"), 1).is_empty());
    }
    #[test]
    fn apply_is_idempotent() {
        let mut lists: Vec<ShoppingList> =
            serde_json::from_str(include_str!("api/testdata/shoppinglists.json")).unwrap();
        let items: Vec<Item> =
            serde_json::from_str(include_str!("api/testdata/items.json")).unwrap();
        let list = lists[0].id;
        let on_list = lists[0].items[0].item.id;
        let off_list = items
            .iter()
            .find(|i| !lists[0].items.iter().any(|e| e.item.id == i.id))
            .unwrap()
            .id;
        let before = lists[0].items.len();

        let check_off = Change::CheckOff {
            list,
            item: on_list,
            at: 1,
        };
        apply(&check_off, &mut lists, &items, None);
        apply(&check_off, &mut lists, &items, None);
        assert_eq!(lists[0].items.len(), before - 1);
        assert_eq!(lists[0].recent_items[0].item.id, on_list);

        let add = Change::Add {
            list,
            item: off_list,
            name: String::new(),
            description: "2 L".into(),
        };
        apply(&add, &mut lists, &items, Some(1));
        apply(&add, &mut lists, &items, Some(1));
        assert_eq!(lists[0].items.len(), before);
        assert_eq!(lists[0].items.last().unwrap().description, "2 L");

        let unknown_list = Change::CheckOff {
            list: -1,
            item: off_list,
            at: 1,
        };
        apply(&unknown_list, &mut lists, &items, None);
        assert_eq!(lists[0].items.len(), before);
    }
}
