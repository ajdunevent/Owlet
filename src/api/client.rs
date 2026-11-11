use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{Value, json};
use url::Url;

use super::error::{Error, Result};
use super::http::{self, Response};
use super::models::{Category, Health, Household, Item, ListInfo, ShoppingList, Timestamp, User};

/// The oldest backend build Owlet supports: KitchenOwl 0.7.10.
pub const MIN_SERVER_BUILD: u32 = 124;

/// KitchenOwl's health route is mounted under a fixed key.
const HEALTH_PATH: &str = "api/health/8M4F88S8ooi4sMbLBfkkV7ctWwgibW6V";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListOrder {
    #[default]
    Alphabetical,
    /// Order learned from the household's shopping history.
    Algorithmic,
}

/// Changes to a catalog item. Fields left as `None` are not sent, so the server keeps them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemUpdate {
    pub name: Option<String>,
    /// `Some(None)` removes the category.
    pub category: Option<Option<i64>>,
    pub icon: Option<String>,
    /// Folds the item with this ID into the updated one.
    pub merge_item_id: Option<i64>,
}

impl ItemUpdate {
    pub(super) fn payload(&self) -> Value {
        let mut payload = json!({});
        if let Some(name) = &self.name {
            payload["name"] = json!(name);
        }
        if let Some(category) = self.category {
            payload["category"] = category.map_or(Value::Null, |id| json!({ "id": id }));
        }
        if let Some(icon) = &self.icon {
            payload["icon"] = json!(icon);
        }
        if let Some(id) = self.merge_item_id {
            payload["merge_item_id"] = json!(id);
        }
        payload
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CategoryUpdate {
    pub name: Option<String>,
    /// New zero-based position; the server shifts the others.
    pub ordering: Option<i64>,
    /// Folds the category with this ID into the updated one.
    pub merge_category_id: Option<i64>,
}

impl CategoryUpdate {
    pub(super) fn payload(&self) -> Value {
        let mut payload = json!({});
        if let Some(name) = &self.name {
            payload["name"] = json!(name);
        }
        if let Some(ordering) = self.ordering {
            payload["ordering"] = json!(ordering);
        }
        if let Some(id) = self.merge_category_id {
            payload["merge_category_id"] = json!(id);
        }
        payload
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignIn {
    /// Long-lived token, for the keyring.
    pub token: String,
    pub user: User,
}

/// Clones share the token.
#[derive(Debug, Clone)]
pub struct Client {
    base: Url,
    token: Rc<RefCell<Option<String>>>,
}

impl Client {
    /// Accepts `https://host`, `https://host/` or `https://host/api`; a missing scheme means https.
    pub fn new(server: &str) -> Result<Self> {
        let trimmed = server.trim();
        let with_scheme = if trimmed.contains("://") {
            trimmed.to_owned()
        } else {
            format!("https://{trimmed}")
        };
        let mut base =
            Url::parse(&with_scheme).map_err(|_| Error::InvalidUrl(server.to_owned()))?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            return Err(Error::InvalidUrl(server.to_owned()));
        }
        let path = base.path().trim_end_matches('/');
        let path = path.strip_suffix("/api").unwrap_or(path).to_owned();
        base.set_path(&format!("{path}/"));
        base.set_query(None);
        base.set_fragment(None);
        Ok(Self {
            base,
            token: Rc::default(),
        })
    }

    /// Server root, ending in `/`.
    pub fn base(&self) -> &Url {
        &self.base
    }

    pub fn token(&self) -> Option<String> {
        self.token.borrow().clone()
    }

    pub fn set_token(&self, token: Option<String>) {
        self.token.replace(token);
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn call(&self, method: &str, path: &str, payload: Option<Value>) -> Result<Response> {
        let token = self.token();
        http::request(method, &self.url(path), token.as_deref(), payload.as_ref())
            .await?
            .check()
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.call("GET", path, None).await?.json()
    }

    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        payload: Value,
    ) -> Result<T> {
        self.call(method, path, Some(payload)).await?.json()
    }

    async fn send_unit(&self, method: &str, path: &str, payload: Option<Value>) -> Result<()> {
        self.call(method, path, payload).await.map(drop)
    }

    pub async fn health(&self) -> Result<Health> {
        let response = http::request("GET", &self.url(HEALTH_PATH), None, None).await?;
        if response.status == 404 {
            return Err(Error::NotKitchenOwl);
        }
        let health: Health = response
            .check()
            .and_then(|r| r.json())
            .map_err(|err| match err {
                Error::Protocol(_) => Error::NotKitchenOwl,
                other => other,
            })?;
        if health.msg != "OK" {
            return Err(Error::NotKitchenOwl);
        }
        Ok(health)
    }

    /// Rejects servers older than [`MIN_SERVER_BUILD`].
    pub async fn check_server(&self) -> Result<Health> {
        let health = self.health().await?;
        if health.version < MIN_SERVER_BUILD {
            return Err(Error::ServerTooOld {
                build: health.version,
            });
        }
        Ok(health)
    }

    /// Logs in, swaps the short session for a long-lived token named `device`, and keeps
    /// that token. The password is not kept.
    pub async fn sign_in(&self, username: &str, password: &str, device: &str) -> Result<SignIn> {
        #[derive(serde::Deserialize)]
        struct Login {
            access_token: String,
            user: User,
        }
        #[derive(serde::Deserialize)]
        struct LongLived {
            longlived_token: String,
        }

        let login: Login = http::request(
            "POST",
            &self.url("api/auth"),
            None,
            Some(&json!({ "username": username, "password": password, "device": device })),
        )
        .await?
        .check()?
        .json()?;

        let session_token = login.access_token;
        let long_lived: LongLived = http::request(
            "POST",
            &self.url("api/auth/llt"),
            Some(&session_token),
            Some(&json!({ "device": device })),
        )
        .await?
        .check()?
        .json()?;

        // Revokes the short session's token family; a failure leaves an unused session behind.
        if let Err(err) = http::request("DELETE", &self.url("api/auth"), Some(&session_token), None)
            .await
            .and_then(Response::check)
        {
            log::warn!("Could not end the login session: {err}");
        }

        self.set_token(Some(long_lived.longlived_token.clone()));
        Ok(SignIn {
            token: long_lived.longlived_token,
            user: login.user,
        })
    }

    /// Revokes the long-lived token on the server; with it as the bearer, the logout route
    /// deletes that token.
    pub async fn sign_out(&self) -> Result<()> {
        let result = self.send_unit("DELETE", "api/auth", None).await;
        self.set_token(None);
        result
    }

    pub async fn user(&self) -> Result<User> {
        self.get("api/user").await
    }

    pub async fn households(&self) -> Result<Vec<Household>> {
        self.get("api/household").await
    }

    pub async fn add_household(&self, name: &str) -> Result<Household> {
        self.send("POST", "api/household", json!({ "name": name }))
            .await
    }

    /// Needs the household's owner or an admin.
    pub async fn rename_household(&self, household: i64, name: &str) -> Result<Household> {
        self.send(
            "POST",
            &format!("api/household/{household}"),
            json!({ "name": name }),
        )
        .await
    }

    /// Removes every member, so the household is gone for everyone. Needs the owner or an
    /// admin.
    pub async fn delete_household(&self, household: i64) -> Result<()> {
        self.send_unit("DELETE", &format!("api/household/{household}"), None)
            .await
    }

    pub async fn leave_household(&self, household: i64, user: i64) -> Result<()> {
        self.send_unit(
            "DELETE",
            &format!("api/household/{household}/member/{user}"),
            None,
        )
        .await
    }

    pub async fn shopping_lists(
        &self,
        household: i64,
        order: ListOrder,
        recent_limit: u32,
    ) -> Result<Vec<ShoppingList>> {
        let orderby = match order {
            ListOrder::Alphabetical => 0,
            ListOrder::Algorithmic => 1,
        };
        self.get(&format!(
            "api/household/{household}/shoppinglist?orderby={orderby}&recent_limit={recent_limit}"
        ))
        .await
    }

    pub async fn add_list(&self, household: i64, name: &str) -> Result<ListInfo> {
        self.send(
            "POST",
            &format!("api/household/{household}/shoppinglist"),
            json!({ "name": name }),
        )
        .await
    }

    pub async fn rename_list(&self, list: i64, name: &str) -> Result<ListInfo> {
        self.send(
            "POST",
            &format!("api/shoppinglist/{list}"),
            json!({ "name": name }),
        )
        .await
    }

    /// The server refuses to delete a household's default (oldest) list.
    pub async fn delete_list(&self, list: i64) -> Result<()> {
        self.send_unit("DELETE", &format!("api/shoppinglist/{list}"), None)
            .await
    }

    /// Adds the catalog item with this name, creating it if needed. An item already on the list
    /// is left as it is, description included.
    pub async fn add_item_by_name(
        &self,
        list: i64,
        name: &str,
        description: Option<&str>,
    ) -> Result<Item> {
        let mut payload = json!({ "name": name });
        if let Some(description) = description {
            payload["description"] = json!(description);
        }
        self.send(
            "POST",
            &format!("api/shoppinglist/{list}/add-item-by-name"),
            payload,
        )
        .await
    }

    pub async fn set_description(&self, list: i64, item: i64, description: &str) -> Result<()> {
        self.send_unit(
            "PUT",
            &format!("api/shoppinglist/{list}/item/{item}"),
            Some(json!({ "description": description })),
        )
        .await
    }

    /// Checks an item off. `removed_at` records when it happened, for changes made offline.
    /// Succeeds when the item is no longer on the list.
    pub async fn remove_item(
        &self,
        list: i64,
        item: i64,
        removed_at: Option<Timestamp>,
    ) -> Result<()> {
        let mut payload = json!({ "item_id": item });
        if let Some(at) = removed_at {
            payload["removed_at"] = json!(at);
        }
        self.send_unit(
            "DELETE",
            &format!("api/shoppinglist/{list}/item"),
            Some(payload),
        )
        .await
    }

    pub async fn remove_items(&self, list: i64, items: &[(i64, Option<Timestamp>)]) -> Result<()> {
        let items: Vec<Value> = items
            .iter()
            .map(|(id, at)| match at {
                Some(at) => json!({ "item_id": id, "removed_at": at }),
                None => json!({ "item_id": id }),
            })
            .collect();
        self.send_unit(
            "DELETE",
            &format!("api/shoppinglist/{list}/items"),
            Some(json!({ "items": items })),
        )
        .await
    }

    pub async fn items(&self, household: i64) -> Result<Vec<Item>> {
        self.get(&format!("api/household/{household}/item")).await
    }

    pub async fn create_item(
        &self,
        household: i64,
        name: &str,
        category: Option<i64>,
    ) -> Result<Item> {
        let mut payload = json!({ "name": name });
        if let Some(id) = category {
            payload["category"] = json!({ "id": id });
        }
        self.send("POST", &format!("api/household/{household}/item"), payload)
            .await
    }

    pub async fn update_item(&self, item: i64, update: &ItemUpdate) -> Result<Item> {
        self.send("POST", &format!("api/item/{item}"), update.payload())
            .await
    }

    pub async fn delete_item(&self, item: i64) -> Result<()> {
        self.send_unit("DELETE", &format!("api/item/{item}"), None)
            .await
    }

    pub async fn categories(&self, household: i64) -> Result<Vec<Category>> {
        self.get(&format!("api/household/{household}/category"))
            .await
    }

    pub async fn add_category(&self, household: i64, name: &str) -> Result<Category> {
        self.send(
            "POST",
            &format!("api/household/{household}/category"),
            json!({ "name": name }),
        )
        .await
    }

    pub async fn update_category(
        &self,
        category: i64,
        update: &CategoryUpdate,
    ) -> Result<Category> {
        self.send(
            "POST",
            &format!("api/category/{category}"),
            update.payload(),
        )
        .await
    }

    /// Items in the category become uncategorised.
    pub async fn delete_category(&self, category: i64) -> Result<()> {
        self.send_unit("DELETE", &format!("api/category/{category}"), None)
            .await
    }
}
