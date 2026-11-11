use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;

use gettextrs::gettext;
use gio::prelude::*;
use glib::subclass::Signal;
use glib::subclass::prelude::*;
use glib::translate::IntoGlib;

use crate::api::{
    self, Category, Client, Frame, Household, Item, ListItem, ListOrder, LiveEvent, ShoppingList,
    User,
};
use crate::cache::{self, Account, Cache, Change, Snapshot};
use crate::config::APP_ID;
use crate::secrets;

const RECENT_LIMIT: u32 = 12;
/// Socket reconnect delays in seconds. Once they run out, a REST request decides whether
/// Owlet is offline.
const LIVE_BACKOFF: [u32; 3] = [1, 3, 8];
/// While the REST API works but the socket does not, the socket is retried this often.
const LIVE_SLOW_RETRY: u32 = 60;
const OFFLINE_RETRY: u32 = 30;
/// Focusing the window refetches data older than this, since renames and category edits
/// are not broadcast.
const STALE_AFTER_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Category,
    Alphabetical,
    Algorithmic,
}

impl SortOrder {
    pub fn from_settings(settings: &gio::Settings) -> Self {
        match settings.string("sort-order").as_str() {
            "alphabetical" => Self::Alphabetical,
            "algorithmic" => Self::Algorithmic,
            _ => Self::Category,
        }
    }
}

pub struct Live {
    connection: soup::WebsocketConnection,
    /// Engine.IO ping interval plus timeout; no frame within it means the link is dead.
    deadline_ms: u64,
    deadline: Option<glib::SourceId>,
    connected: bool,
}

mod imp {
    use super::*;

    #[derive(glib::Properties)]
    #[properties(wrapper_type = super::Store)]
    pub struct Store {
        pub settings: gio::Settings,
        pub client: RefCell<Option<Client>>,
        pub cache: RefCell<Option<Cache>>,
        pub user: RefCell<Option<User>>,
        pub households: RefCell<Vec<Household>>,
        pub lists: RefCell<Vec<ShoppingList>>,
        pub items: RefCell<Vec<Item>>,
        pub categories: RefCell<Vec<Category>>,
        /// Time of the last successful fetch, in milliseconds.
        pub synced_at: Cell<i64>,
        /// Last successful exchange with the server, in milliseconds.
        pub last_contact: Cell<i64>,
        /// (list, item) pairs with a change still in the queue.
        pub waiting: RefCell<HashSet<(i64, i64)>>,
        pub pending: Cell<u32>,
        pub syncing: Cell<bool>,
        pub live: RefCell<Option<Live>>,
        /// Bumped whenever the socket is replaced, so callbacks from an old one are ignored.
        pub live_generation: Cell<u64>,
        pub live_failures: Cell<usize>,
        pub live_ever_connected: Cell<bool>,
        pub live_retry: RefCell<Option<glib::SourceId>>,
        pub offline_retry: RefCell<Option<glib::SourceId>>,
        #[property(get)]
        pub busy: Cell<bool>,
        #[property(get)]
        pub signed_in: Cell<bool>,
        /// False until the stored session has been checked at startup.
        #[property(get)]
        pub ready: Cell<bool>,
        #[property(get)]
        pub online: Cell<bool>,
        /// The server rejected the saved token; the login page explains why it is showing.
        #[property(get)]
        pub session_expired: Cell<bool>,
    }

    impl Default for Store {
        fn default() -> Self {
            Self {
                settings: gio::Settings::new(APP_ID),
                client: Default::default(),
                cache: Default::default(),
                user: Default::default(),
                households: Default::default(),
                lists: Default::default(),
                items: Default::default(),
                categories: Default::default(),
                synced_at: Default::default(),
                last_contact: Default::default(),
                waiting: Default::default(),
                pending: Default::default(),
                syncing: Default::default(),
                live: Default::default(),
                live_generation: Default::default(),
                live_failures: Default::default(),
                live_ever_connected: Default::default(),
                live_retry: Default::default(),
                offline_retry: Default::default(),
                busy: Default::default(),
                signed_in: Default::default(),
                ready: Default::default(),
                online: Cell::new(true),
                session_expired: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Store {
        const NAME: &'static str = "OwletStore";
        type Type = super::Store;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Store {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    Signal::builder("changed").build(),
                    Signal::builder("message")
                        .param_types([String::static_type()])
                        .build(),
                ]
            })
        }

        fn constructed(&self) {
            self.parent_constructed();
            match Cache::open_default() {
                Ok(cache) => {
                    self.cache.replace(Some(cache));
                }
                Err(err) => log::warn!("Could not open the cache: {err}"),
            }
            let store = self.obj();
            gio::NetworkMonitor::default().connect_network_changed(glib::clone!(
                #[weak]
                store,
                move |_, available| store.on_network_changed(available)
            ));
        }
    }
}

glib::wrapper! {
    pub struct Store(ObjectSubclass<imp::Store>);
}

impl Default for Store {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl Store {
    pub fn settings(&self) -> gio::Settings {
        self.imp().settings.clone()
    }

    pub fn client(&self) -> Option<Client> {
        self.imp().client.borrow().clone()
    }

    pub fn user(&self) -> Option<User> {
        self.imp().user.borrow().clone()
    }

    pub fn households(&self) -> Vec<Household> {
        self.imp().households.borrow().clone()
    }

    pub fn household(&self) -> Option<Household> {
        let id = i64::from(self.imp().settings.int("household"));
        let households = self.imp().households.borrow();
        households
            .iter()
            .find(|h| h.id == id)
            .or_else(|| households.first())
            .cloned()
    }

    pub fn has_data(&self) -> bool {
        !self.imp().households.borrow().is_empty()
    }

    pub fn lists(&self) -> Vec<ShoppingList> {
        self.imp().lists.borrow().clone()
    }

    /// The last opened list on this device, or the household's default list.
    pub fn current_list(&self) -> Option<ShoppingList> {
        let lists = self.imp().lists.borrow();
        let saved = i64::from(self.imp().settings.int("shopping-list"));
        let default = self.default_list_id();
        lists
            .iter()
            .find(|l| l.id == saved)
            .or_else(|| lists.iter().find(|l| Some(l.id) == default))
            .or_else(|| lists.first())
            .cloned()
    }

    pub fn default_list_id(&self) -> Option<i64> {
        self.household()
            .and_then(|h| h.default_shopping_list)
            .map(|l| l.id)
    }

    pub fn items(&self) -> Vec<Item> {
        self.imp().items.borrow().clone()
    }

    pub fn categories(&self) -> Vec<Category> {
        self.imp().categories.borrow().clone()
    }

    pub fn server_url(&self) -> Option<String> {
        self.client().map(|c| c.base().to_string())
    }

    pub fn last_contact(&self) -> i64 {
        self.imp().last_contact.get()
    }

    fn touch(&self) {
        self.imp().last_contact.set(glib::real_time() / 1000);
    }

    /// The item has a change on this list that has not reached the server yet.
    pub fn is_waiting(&self, list: i64, item: i64) -> bool {
        self.imp().waiting.borrow().contains(&(list, item))
    }

    fn notify_message(&self, text: &str) {
        self.emit_by_name::<()>("message", &[&text]);
    }

    pub fn show_message(&self, text: &str) {
        self.notify_message(text);
    }

    fn emit_changed(&self) {
        self.emit_by_name::<()>("changed", &[]);
    }

    fn set_busy_by(&self, delta: i32) {
        let imp = self.imp();
        let pending = imp.pending.get().saturating_add_signed(delta);
        imp.pending.set(pending);
        if imp.busy.get() != (pending > 0) {
            imp.busy.set(pending > 0);
            self.notify_busy();
        }
    }

    fn set_signed_in(&self, signed_in: bool) {
        if self.imp().signed_in.get() != signed_in {
            self.imp().signed_in.set(signed_in);
            self.notify_signed_in();
        }
    }

    fn set_ready(&self) {
        if !self.imp().ready.get() {
            self.imp().ready.set(true);
            self.notify_ready();
        }
    }

    fn set_session_expired(&self, expired: bool) {
        if self.imp().session_expired.get() != expired {
            self.imp().session_expired.set(expired);
            self.notify_session_expired();
        }
    }

    fn account(&self) -> Option<Account> {
        let settings = &self.imp().settings;
        let server = settings.string("server-url").to_string();
        let username = settings.string("username").to_string();
        (!server.is_empty() && !username.is_empty()).then_some(Account { server, username })
    }

    /// Shows the cached data at once, then refreshes from the server.
    pub async fn restore(&self) {
        let Some(account) = self.account() else {
            self.set_ready();
            return;
        };
        let token = match secrets::load_token(&account.server, &account.username).await {
            Ok(token) => token,
            Err(err) => {
                log::warn!("Could not read the keyring: {err}");
                None
            }
        };
        let (Ok(client), Some(token)) = (Client::new(&account.server), token) else {
            self.set_ready();
            return;
        };
        client.set_token(Some(token));
        self.imp().client.replace(Some(client));
        let cached = self.with_cache(|c| c.load(&account)).flatten();
        if let Some(snapshot) = cached {
            self.show_snapshot(snapshot);
            self.set_signed_in(true);
            self.set_ready();
            self.refresh().await;
        } else {
            self.refresh().await;
            // A revoked token clears the client during the refresh.
            self.set_signed_in(self.client().is_some());
            self.set_ready();
        }
    }

    pub async fn sign_in(
        &self,
        server: &str,
        username: &str,
        password: &str,
    ) -> Result<(), api::Error> {
        let client = Client::new(server)?;
        client.check_server().await?;
        let device = format!("Owlet on {}", glib::host_name());
        let signed_in = client.sign_in(username, password, &device).await?;
        let base = client.base().to_string();
        let username = signed_in.user.username.clone();
        if let Err(err) = secrets::store_token(&base, &username, &signed_in.token).await {
            log::warn!("Could not save the token to the keyring: {err}");
        }
        let settings = &self.imp().settings;
        let _ = settings.set_string("server-url", &base);
        let _ = settings.set_string("username", &username);
        self.imp().user.replace(Some(signed_in.user));
        self.imp().client.replace(Some(client));
        self.set_session_expired(false);
        // Fetched before showing the main view, so it never appears empty.
        self.refresh().await;
        self.set_signed_in(self.client().is_some());
        Ok(())
    }

    pub async fn sign_out(&self) {
        if let Some(client) = self.client() {
            if let Err(err) = client.sign_out().await {
                log::warn!("Could not revoke the token on the server: {err}");
            }
            let username = self.imp().settings.string("username").to_string();
            if let Err(err) = secrets::clear_token(client.base().as_str(), &username).await {
                log::warn!("Could not remove the token from the keyring: {err}");
            }
        }
        for key in ["server-url", "username"] {
            let _ = self.imp().settings.set_string(key, "");
        }
        self.end_session();
    }

    /// Keeps the address and username for the login page, and the queue for when the same
    /// account logs in again.
    async fn expire_session(&self) {
        if let Some(account) = self.account()
            && let Err(err) = secrets::clear_token(&account.server, &account.username).await
        {
            log::warn!("Could not remove the token from the keyring: {err}");
        }
        self.set_session_expired(true);
        self.end_session();
    }

    fn end_session(&self) {
        let imp = self.imp();
        for key in ["household", "shopping-list"] {
            let _ = imp.settings.set_int(key, 0);
        }
        self.stop_live();
        if let Some(source) = imp.offline_retry.take() {
            source.remove();
        }
        imp.client.replace(None);
        imp.user.replace(None);
        imp.households.replace(Vec::new());
        imp.lists.replace(Vec::new());
        imp.items.replace(Vec::new());
        imp.categories.replace(Vec::new());
        imp.waiting.borrow_mut().clear();
        imp.synced_at.set(0);
        imp.last_contact.set(0);
        self.set_online(true);
        self.set_signed_in(false);
        self.emit_changed();
    }

    fn set_online(&self, online: bool) {
        let imp = self.imp();
        if imp.online.get() == online {
            return;
        }
        imp.online.set(online);
        if online {
            if let Some(source) = imp.offline_retry.take() {
                source.remove();
            }
        } else {
            self.stop_live();
            // Keeps the last contact time for a restart while still offline.
            self.save_snapshot();
            let source = glib::timeout_add_seconds_local(
                OFFLINE_RETRY,
                glib::clone!(
                    #[weak(rename_to = store)]
                    self,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        glib::spawn_future_local(async move { store.refresh().await });
                        glib::ControlFlow::Continue
                    }
                ),
            );
            imp.offline_retry.replace(Some(source));
        }
        self.notify_online();
    }

    fn on_network_changed(&self, available: bool) {
        if self.client().is_none() {
            return;
        }
        if !available {
            self.set_online(false);
        } else if !self.online() {
            glib::spawn_future_local(glib::clone!(
                #[weak(rename_to = store)]
                self,
                async move { store.refresh().await }
            ));
        }
    }

    pub fn refresh_if_stale(&self) {
        let age = glib::real_time() / 1000 - self.imp().synced_at.get();
        if self.signed_in() && !self.busy() && age > STALE_AFTER_MS {
            glib::spawn_future_local(glib::clone!(
                #[weak(rename_to = store)]
                self,
                async move { store.refresh().await }
            ));
        }
    }

    fn with_cache<T>(&self, f: impl FnOnce(&Cache) -> T) -> Option<T> {
        self.imp().cache.borrow().as_ref().map(f)
    }

    fn household_id(&self) -> Option<i64> {
        self.household().map(|h| h.id)
    }

    fn queued(&self) -> Vec<cache::Queued> {
        match (self.account(), self.household_id()) {
            (Some(account), Some(household)) => self
                .with_cache(|c| c.queued(&account, household))
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn reload_waiting(&self) {
        let waiting = self
            .queued()
            .iter()
            .map(|q| (q.change.list(), q.change.item()))
            .collect();
        self.imp().waiting.replace(waiting);
    }

    fn show_snapshot(&self, snapshot: Snapshot) {
        let imp = self.imp();
        imp.user.replace(snapshot.user);
        imp.households.replace(snapshot.households);
        let mut lists = snapshot.lists;
        let user = imp.user.borrow().as_ref().map(|u| u.id);
        imp.items.replace(snapshot.items);
        imp.categories.replace(snapshot.categories);
        imp.synced_at.set(snapshot.synced_at);
        imp.last_contact
            .set(snapshot.last_contact.max(snapshot.synced_at));
        for queued in self.queued() {
            cache::apply(&queued.change, &mut lists, &imp.items.borrow(), user);
        }
        imp.lists.replace(lists);
        self.reload_waiting();
        self.emit_changed();
    }

    fn save_snapshot(&self) {
        let imp = self.imp();
        let Some(account) = self.account() else {
            return;
        };
        let snapshot = Snapshot {
            user: imp.user.borrow().clone(),
            households: imp.households.borrow().clone(),
            lists: imp.lists.borrow().clone(),
            items: imp.items.borrow().clone(),
            categories: imp.categories.borrow().clone(),
            synced_at: imp.synced_at.get(),
            last_contact: imp.last_contact.get(),
        };
        if let Some(Err(err)) = self.with_cache(|c| c.save(&account, &snapshot)) {
            log::warn!("Could not save the cache: {err}");
        }
    }

    async fn fetch(&self, client: &Client) -> api::Result<Snapshot> {
        let (user, households) = futures_util::join!(client.user(), client.households());
        let (user, households) = (user?, households?);
        let id = i64::from(self.imp().settings.int("household"));
        let Some(household) = households
            .iter()
            .find(|h| h.id == id)
            .or_else(|| households.first())
            .map(|h| h.id)
        else {
            return Ok(Snapshot {
                user: Some(user),
                households,
                synced_at: glib::real_time() / 1000,
                ..Default::default()
            });
        };
        // Lists always come in the learned order; the other sort orders are applied locally.
        let (lists, items, categories) = futures_util::join!(
            client.shopping_lists(household, ListOrder::Algorithmic, RECENT_LIMIT),
            client.items(household),
            client.categories(household),
        );
        Ok(Snapshot {
            user: Some(user),
            households,
            lists: lists?,
            items: items?,
            categories: categories?,
            synced_at: glib::real_time() / 1000,
            last_contact: glib::real_time() / 1000,
        })
    }

    /// Fetches everything, sends queued changes, and refetches if any were sent.
    pub async fn refresh(&self) {
        let imp = self.imp();
        let Some(client) = self.client() else {
            return;
        };
        if imp.syncing.replace(true) {
            return;
        }
        self.set_busy_by(1);
        let mut result = self.fetch(&client).await;
        if let Ok(snapshot) = &result
            && self.replay(&client, snapshot).await
        {
            result = self.fetch(&client).await;
        }
        self.set_busy_by(-1);
        imp.syncing.set(false);
        match result {
            Ok(snapshot) => {
                self.touch();
                self.set_online(true);
                self.show_snapshot(snapshot);
                self.save_snapshot();
                self.start_live();
            }
            Err(api::Error::Unauthorized) => self.expire_session().await,
            Err(err) if err.is_network() => {
                log::info!("Offline: {err}");
                self.set_online(false);
            }
            Err(err) => {
                log::warn!("Refresh failed: {err}");
                self.notify_message(&gettext("Couldn't load data from the server"));
            }
        }
    }

    /// Sends queued changes in order against a fresh `snapshot`. Returns whether any reached
    /// the server. A network failure stops the replay and keeps the rest of the queue.
    async fn replay(&self, client: &Client, snapshot: &Snapshot) -> bool {
        let queued = self.queued();
        let mut sent = false;
        for queued in queued {
            let list = snapshot.lists.iter().find(|l| l.id == queued.change.list());
            let item = queued.change.item();
            let result = match (&queued.change, list) {
                (_, None) => None,
                (Change::Add { description, .. }, Some(list)) => {
                    // Sent by the item's current name, which may have changed since.
                    let name = snapshot
                        .items
                        .iter()
                        .find(|i| i.id == item)
                        .map(|i| &i.name);
                    match name {
                        Some(name) if !list.items.iter().any(|i| i.item.id == item) => Some(
                            client
                                .add_item_by_name(
                                    list.id,
                                    name,
                                    Some(description.as_str()).filter(|d| !d.is_empty()),
                                )
                                .await
                                .map(drop),
                        ),
                        _ => None,
                    }
                }
                (Change::CheckOff { at, .. }, Some(list)) => list
                    .items
                    .iter()
                    .any(|i| i.item.id == item)
                    .then_some(client.remove_item(list.id, item, Some(*at)).await),
            };
            match result {
                Some(Err(err)) if err.is_network() || err == api::Error::Unauthorized => break,
                Some(Err(err)) => log::warn!("Dropping a queued change: {err}"),
                Some(Ok(())) => sent = true,
                None => log::info!("Dropping a queued change that is already done"),
            }
            if let Some(Err(err)) = self.with_cache(|c| c.remove(queued.id)) {
                log::warn!("Could not update the queue: {err}");
            }
        }
        self.reload_waiting();
        sent
    }

    pub async fn switch_household(&self, id: i64) {
        let settings = &self.imp().settings;
        if i64::from(settings.int("household")) == id {
            return;
        }
        let _ = settings.set_int("household", id as i32);
        let _ = settings.set_int("shopping-list", 0);
        self.refresh().await;
    }

    pub fn open_list(&self, id: i64) {
        let _ = self.imp().settings.set_int("shopping-list", id as i32);
        self.emit_changed();
    }

    fn live_connected(&self) -> bool {
        self.imp()
            .live
            .borrow()
            .as_ref()
            .is_some_and(|live| live.connected)
    }

    fn start_live(&self) {
        let imp = self.imp();
        if imp.live.borrow().is_some() || imp.live_retry.borrow().is_some() || !self.online() {
            return;
        }
        let Some(client) = self.client() else {
            return;
        };
        log::debug!("Connecting live updates");
        let generation = imp.live_generation.get() + 1;
        imp.live_generation.set(generation);
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = store)]
            self,
            async move {
                let result = client.connect_live().await;
                if store.imp().live_generation.get() != generation {
                    if let Ok(connection) = result {
                        connection.close(1000, None);
                    }
                    return;
                }
                match result {
                    Ok(connection) => {
                        log::debug!("Live updates socket open");
                        store.attach_live(connection, generation);
                    }
                    Err(err) => {
                        log::info!("Live updates unavailable: {err}");
                        store.on_live_down(generation);
                    }
                }
            }
        ));
    }

    fn attach_live(&self, connection: soup::WebsocketConnection, generation: u64) {
        connection.connect_message(glib::clone!(
            #[weak(rename_to = store)]
            self,
            move |connection, kind, bytes| {
                if kind == soup::WebsocketDataType::Text.into_glib()
                    && let Ok(text) = std::str::from_utf8(bytes)
                {
                    store.on_live_frame(connection, generation, api::parse_frame(text));
                }
            }
        ));
        connection.connect_closed(glib::clone!(
            #[weak(rename_to = store)]
            self,
            move |_| store.on_live_down(generation)
        ));
        self.imp().live.replace(Some(Live {
            connection,
            deadline_ms: 45_000,
            deadline: None,
            connected: false,
        }));
        self.arm_live_deadline(generation);
    }

    fn arm_live_deadline(&self, generation: u64) {
        let mut live = self.imp().live.borrow_mut();
        let Some(live) = live.as_mut() else {
            return;
        };
        if let Some(source) = live.deadline.take() {
            source.remove();
        }
        live.deadline = Some(glib::timeout_add_local_once(
            Duration::from_millis(live.deadline_ms),
            glib::clone!(
                #[weak(rename_to = store)]
                self,
                move || {
                    if let Some(live) = store.imp().live.borrow_mut().as_mut() {
                        live.deadline = None;
                    }
                    log::info!("Live updates timed out");
                    store.on_live_down(generation);
                }
            ),
        ));
    }

    fn on_live_frame(&self, connection: &soup::WebsocketConnection, generation: u64, frame: Frame) {
        if self.imp().live_generation.get() != generation {
            return;
        }
        self.arm_live_deadline(generation);
        self.touch();
        match frame {
            Frame::Open {
                ping_interval,
                ping_timeout,
            } => {
                if let Some(live) = self.imp().live.borrow_mut().as_mut() {
                    live.deadline_ms = u64::from(ping_interval) + u64::from(ping_timeout);
                }
                connection.send_text(api::CONNECT);
            }
            Frame::Ping => connection.send_text(api::PONG),
            Frame::Connected => self.on_live_connected(),
            Frame::ConnectError(reason) => {
                log::info!("Live updates refused: {reason}");
                connection.close(1000, None);
            }
            Frame::Close => connection.close(1000, None),
            Frame::Event(event) => self.apply_event(event),
            Frame::Other => {}
        }
    }

    fn on_live_connected(&self) {
        let imp = self.imp();
        if let Some(live) = imp.live.borrow_mut().as_mut() {
            live.connected = true;
        }
        log::debug!("Live updates connected");
        imp.live_failures.set(0);
        // Events sent while the socket was down are lost, so a reconnect refetches.
        if imp.live_ever_connected.replace(true) {
            glib::spawn_future_local(glib::clone!(
                #[weak(rename_to = store)]
                self,
                async move { store.refresh().await }
            ));
        }
    }

    fn on_live_down(&self, generation: u64) {
        let imp = self.imp();
        if imp.live_generation.get() != generation {
            return;
        }
        imp.live_generation.set(generation + 1);
        log::debug!("Live updates disconnected");
        if let Some(live) = imp.live.take() {
            if let Some(source) = live.deadline {
                source.remove();
            }
            live.connection.close(1000, None);
        }
        if self.client().is_none() || !self.online() {
            return;
        }
        let failures = imp.live_failures.get();
        imp.live_failures.set(failures + 1);
        let delay = match LIVE_BACKOFF.get(failures) {
            Some(&delay) => delay,
            None => {
                if failures == LIVE_BACKOFF.len() {
                    glib::spawn_future_local(glib::clone!(
                        #[weak(rename_to = store)]
                        self,
                        async move { store.refresh().await }
                    ));
                }
                LIVE_SLOW_RETRY
            }
        };
        let source = glib::timeout_add_seconds_local_once(
            delay,
            glib::clone!(
                #[weak(rename_to = store)]
                self,
                move || {
                    store.imp().live_retry.replace(None);
                    store.start_live();
                }
            ),
        );
        imp.live_retry.replace(Some(source));
    }

    fn stop_live(&self) {
        let imp = self.imp();
        imp.live_generation.set(imp.live_generation.get() + 1);
        if let Some(source) = imp.live_retry.take() {
            source.remove();
        }
        if let Some(live) = imp.live.take() {
            if let Some(source) = live.deadline {
                source.remove();
            }
            live.connection.close(1000, None);
        }
        imp.live_failures.set(0);
        imp.live_ever_connected.set(false);
    }

    fn apply_event(&self, event: LiveEvent) {
        let imp = self.imp();
        if Some(event.household_id()) != self.household_id() {
            return;
        }
        match event {
            LiveEvent::ItemAdded { list, item } => {
                let catalog = imp
                    .items
                    .borrow()
                    .iter()
                    .find(|i| i.id == item.item.id)
                    .cloned();
                // Socket items carry no category, so the catalog's copy is preferred.
                let entry = ListItem {
                    item: catalog.clone().unwrap_or_else(|| item.item.clone()),
                    ..*item
                };
                if catalog.is_none() {
                    imp.items.borrow_mut().push(entry.item.clone());
                }
                let mut lists = imp.lists.borrow_mut();
                let Some(list) = lists.iter_mut().find(|l| l.id == list.id) else {
                    return;
                };
                match list.items.iter_mut().find(|i| i.item.id == entry.item.id) {
                    Some(existing) => *existing = entry,
                    None => list.items.push(entry),
                }
            }
            LiveEvent::ItemRemoved { list, item_id } => {
                let mut lists = imp.lists.borrow_mut();
                let Some(list) = lists.iter_mut().find(|l| l.id == list.id) else {
                    return;
                };
                if let Some(position) = list.items.iter().position(|i| i.item.id == item_id) {
                    let entry = list.items.remove(position);
                    list.recent_items.retain(|r| r.item.id != item_id);
                    list.recent_items.insert(0, entry);
                }
            }
            LiveEvent::ListAdded(info) => {
                let mut lists = imp.lists.borrow_mut();
                if !lists.iter().any(|l| l.id == info.id) {
                    lists.push(ShoppingList {
                        id: info.id,
                        name: info.name,
                        household_id: info.household_id,
                        items: Vec::new(),
                        recent_items: Vec::new(),
                    });
                }
            }
            LiveEvent::ListDeleted(info) => {
                imp.lists.borrow_mut().retain(|l| l.id != info.id);
            }
        }
        self.save_snapshot();
        self.emit_changed();
    }

    fn needs_connection(&self) {
        self.notify_message(&gettext("This change needs a connection"));
    }

    /// Records a change made offline and shows it applied.
    fn enqueue(&self, change: Change) {
        let imp = self.imp();
        let (Some(account), Some(household)) = (self.account(), self.household_id()) else {
            return;
        };
        if let Some(Err(err)) = self.with_cache(|c| c.push(&account, household, &change)) {
            log::warn!("Could not queue the change: {err}");
        }
        let user = imp.user.borrow().as_ref().map(|u| u.id);
        cache::apply(
            &change,
            &mut imp.lists.borrow_mut(),
            &imp.items.borrow(),
            user,
        );
        self.reload_waiting();
        self.save_snapshot();
        self.emit_changed();
    }

    /// Undoing a change that is still queued removes it from the queue instead of queuing
    /// its opposite.
    fn cancel_queued(&self, list: i64, item: i64, add: bool) -> bool {
        let found = self.queued().into_iter().rev().find(|q| {
            q.change.list() == list
                && q.change.item() == item
                && matches!(q.change, Change::Add { .. }) == add
        });
        let Some(found) = found else {
            return false;
        };
        if let Some(Err(err)) = self.with_cache(|c| c.remove(found.id)) {
            log::warn!("Could not update the queue: {err}");
        }
        self.reload_waiting();
        true
    }

    /// Offline, only names already in the catalog can be added.
    pub fn can_add(&self, name: &str) -> bool {
        self.online() || self.find_item(name).is_some()
    }

    fn find_item(&self, name: &str) -> Option<Item> {
        let wanted = name.trim().to_lowercase();
        self.imp()
            .items
            .borrow()
            .iter()
            .find(|i| i.name.to_lowercase() == wanted)
            .cloned()
    }

    pub async fn add_to_list(&self, list: i64, name: &str, description: Option<&str>) -> bool {
        let imp = self.imp();
        let description = description.unwrap_or_default().to_owned();
        let known = self.find_item(name);
        if let Some(item) = &known
            && self.cancel_queued(list, item.id, false)
        {
            let user = imp.user.borrow().as_ref().map(|u| u.id);
            let change = Change::Add {
                list,
                item: item.id,
                name: item.name.clone(),
                description: description.clone(),
            };
            cache::apply(
                &change,
                &mut imp.lists.borrow_mut(),
                &imp.items.borrow(),
                user,
            );
            self.save_snapshot();
            self.emit_changed();
            return true;
        }
        let queue_add = |store: &Self| match &known {
            Some(item) => {
                store.enqueue(Change::Add {
                    list,
                    item: item.id,
                    name: item.name.clone(),
                    description: description.clone(),
                });
                true
            }
            None => {
                store.needs_connection();
                false
            }
        };
        let Some(client) = self.client().filter(|_| self.online()) else {
            return queue_add(self);
        };
        self.set_busy_by(1);
        let result = client
            .add_item_by_name(
                list,
                name,
                Some(description.as_str()).filter(|d| !d.is_empty()),
            )
            .await;
        self.set_busy_by(-1);
        match result {
            Ok(item) => {
                self.touch();
                if !imp.items.borrow().iter().any(|i| i.id == item.id) {
                    imp.items.borrow_mut().push(item.clone());
                }
                let user = imp.user.borrow().as_ref().map(|u| u.id);
                let change = Change::Add {
                    list,
                    item: item.id,
                    name: item.name,
                    description,
                };
                cache::apply(
                    &change,
                    &mut imp.lists.borrow_mut(),
                    &imp.items.borrow(),
                    user,
                );
                self.save_snapshot();
                self.emit_changed();
                if !self.live_connected() {
                    self.refresh().await;
                }
                true
            }
            Err(err) if err.is_network() => {
                self.set_online(false);
                queue_add(self)
            }
            Err(err) => {
                self.report_failure(&gettext("Couldn't add the item"), err)
                    .await;
                false
            }
        }
    }

    /// Removes the items from the screen at once, then from the server, or queues them.
    pub async fn check_off(&self, list: i64, items: &[i64]) -> bool {
        let imp = self.imp();
        let at = glib::real_time() / 1000;
        let changes: Vec<Change> = items
            .iter()
            .map(|&item| Change::CheckOff { list, item, at })
            .collect();
        let queue_all = |store: &Self| {
            for change in &changes {
                if !store.cancel_queued(list, change.item(), true) {
                    store.enqueue(change.clone());
                }
            }
            store.reload_waiting();
            store.save_snapshot();
            store.emit_changed();
            true
        };
        for change in &changes {
            cache::apply(
                change,
                &mut imp.lists.borrow_mut(),
                &imp.items.borrow(),
                None,
            );
        }
        self.emit_changed();
        let Some(client) = self.client().filter(|_| self.online()) else {
            return queue_all(self);
        };
        let removals: Vec<(i64, Option<i64>)> = items.iter().map(|&id| (id, Some(at))).collect();
        self.set_busy_by(1);
        let result = if let [(id, at)] = removals.as_slice() {
            client.remove_item(list, *id, *at).await
        } else {
            client.remove_items(list, &removals).await
        };
        self.set_busy_by(-1);
        match result {
            Ok(()) => {
                self.touch();
                self.save_snapshot();
                if !self.live_connected() {
                    self.refresh().await;
                }
                true
            }
            Err(err) if err.is_network() => {
                self.set_online(false);
                queue_all(self)
            }
            Err(err) => {
                self.report_failure(&gettext("Couldn't check off the item"), err)
                    .await;
                self.refresh().await;
                false
            }
        }
    }

    async fn report_failure(&self, failure: &str, err: api::Error) {
        match err {
            api::Error::Unauthorized => self.expire_session().await,
            err if err.is_network() => {
                self.set_online(false);
                self.needs_connection();
            }
            err => {
                log::warn!("{failure}: {err}");
                self.notify_message(failure);
            }
        }
    }

    /// Runs a change that needs the server, then refetches so the screen matches it.
    async fn change<T>(
        &self,
        failure: &str,
        call: impl AsyncFnOnce(Client, i64) -> api::Result<T>,
    ) -> Option<T> {
        let client = self.client()?;
        let household = self.household_id().unwrap_or_default();
        if !self.online() {
            self.needs_connection();
            return None;
        }
        self.set_busy_by(1);
        let result = call(client, household).await;
        self.set_busy_by(-1);
        let value = match result {
            Ok(value) => {
                self.touch();
                Some(value)
            }
            Err(err) => {
                self.report_failure(failure, err).await;
                None
            }
        };
        if self.online() {
            self.refresh().await;
        }
        value
    }

    pub async fn set_description(&self, list: i64, item: i64, description: &str) -> bool {
        self.change(&gettext("Couldn't save the description"), async |c, _| {
            c.set_description(list, item, description).await
        })
        .await
        .is_some()
    }

    pub async fn update_item(&self, item: i64, update: api::ItemUpdate) -> bool {
        self.change(&gettext("Couldn't save the item"), async |c, _| {
            c.update_item(item, &update).await
        })
        .await
        .is_some()
    }

    pub async fn delete_item(&self, item: i64) -> bool {
        self.change(&gettext("Couldn't delete the item"), async |c, _| {
            c.delete_item(item).await
        })
        .await
        .is_some()
    }

    pub async fn create_item(&self, name: &str, category: Option<i64>) -> Option<Item> {
        self.change(&gettext("Couldn't create the item"), async |c, h| {
            c.create_item(h, name, category).await
        })
        .await
    }

    pub async fn add_list(&self, name: &str) -> Option<i64> {
        let list = self
            .change(&gettext("Couldn't add the list"), async |c, h| {
                c.add_list(h, name).await
            })
            .await?;
        self.open_list(list.id);
        Some(list.id)
    }

    /// Owners and admins of a household, and server admins, may rename or delete it.
    pub fn can_manage_household(&self, household: &Household) -> bool {
        let Some(user) = self.user() else {
            return false;
        };
        user.admin
            || household
                .member
                .iter()
                .any(|m| m.id == user.id && (m.owner || m.admin))
    }

    pub async fn add_household(&self, name: &str) -> bool {
        let Some(household) = self
            .change(&gettext("Couldn't add the household"), async |c, _| {
                c.add_household(name).await
            })
            .await
        else {
            return false;
        };
        // The socket joins household rooms when it connects, so it reconnects to hear
        // the new one.
        self.stop_live();
        self.switch_household(household.id).await;
        true
    }

    pub async fn rename_household(&self, household: i64, name: &str) -> bool {
        self.change(&gettext("Couldn't rename the household"), async |c, _| {
            c.rename_household(household, name).await
        })
        .await
        .is_some()
    }

    pub async fn delete_household(&self, household: i64) -> bool {
        let done = self
            .change(&gettext("Couldn't delete the household"), async |c, _| {
                c.delete_household(household).await
            })
            .await
            .is_some();
        self.settle_household();
        done
    }

    pub async fn leave_household(&self, household: i64) -> bool {
        let Some(user) = self.user() else {
            return false;
        };
        let done = self
            .change(&gettext("Couldn't leave the household"), async |c, _| {
                c.leave_household(household, user.id).await
            })
            .await
            .is_some();
        self.settle_household();
        done
    }

    /// After the open household goes away, the refetch falls back to the first remaining
    /// one; the saved choice is updated to match.
    fn settle_household(&self) {
        let settings = &self.imp().settings;
        let shown = self.household_id().unwrap_or_default();
        if i64::from(settings.int("household")) != shown {
            let _ = settings.set_int("household", shown as i32);
            let _ = settings.set_int("shopping-list", 0);
            self.emit_changed();
        }
    }

    pub async fn rename_list(&self, list: i64, name: &str) -> bool {
        self.change(&gettext("Couldn't rename the list"), async |c, _| {
            c.rename_list(list, name).await
        })
        .await
        .is_some()
    }

    pub async fn delete_list(&self, list: i64) -> bool {
        self.change(&gettext("Couldn't delete the list"), async |c, _| {
            c.delete_list(list).await
        })
        .await
        .is_some()
    }

    pub async fn add_category(&self, name: &str) -> bool {
        self.change(&gettext("Couldn't add the category"), async |c, h| {
            c.add_category(h, name).await
        })
        .await
        .is_some()
    }

    pub async fn update_category(&self, category: i64, update: api::CategoryUpdate) -> bool {
        self.change(&gettext("Couldn't change the category"), async |c, _| {
            c.update_category(category, &update).await
        })
        .await
        .is_some()
    }

    pub async fn delete_category(&self, category: i64) -> bool {
        self.change(&gettext("Couldn't delete the category"), async |c, _| {
            c.delete_category(category).await
        })
        .await
        .is_some()
    }

    /// Categories in the server's order: position, then name for ties.
    pub fn ordered_categories(&self) -> Vec<Category> {
        let mut categories = self.categories();
        categories.sort_by(|a, b| {
            a.ordering
                .cmp(&b.ordering)
                .then_with(|| a.name.cmp(&b.name))
        });
        categories
    }
}
