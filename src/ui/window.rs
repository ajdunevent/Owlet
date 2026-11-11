use std::cell::Cell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{CompositeTemplate, gio, glib};

use crate::application::Application;
use crate::config::APP_ID;
use crate::store::Store;
use crate::ui::{CatalogPage, ListPage, LoginPage};
use crate::ui::{manage_dialogs, when, widgets};

mod imp {
    use super::*;

    #[derive(CompositeTemplate)]
    #[template(resource = "/net/dunevent/Owlet/ui/window.ui")]
    pub struct Window {
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub login_page: TemplateChild<LoginPage>,
        #[template_child]
        pub offline_status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub split_view: TemplateChild<adw::OverlaySplitView>,
        #[template_child]
        pub household_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub household_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub nav_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub refresh_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub title_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub page_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub list_switcher: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub list_switcher_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub offline_badge: TemplateChild<gtk::Label>,
        #[template_child]
        pub last_refresh: TemplateChild<gtk::Box>,
        #[template_child]
        pub last_refresh_time: TemplateChild<gtk::Label>,
        #[template_child]
        pub pages: TemplateChild<adw::ViewStack>,
        #[template_child]
        pub list_page: TemplateChild<ListPage>,
        #[template_child]
        pub catalog_page: TemplateChild<CatalogPage>,
        #[template_child]
        pub households_menu: TemplateChild<gio::Menu>,
        #[template_child]
        pub households_submenu: TemplateChild<gio::Menu>,
        #[template_child]
        pub lists_menu: TemplateChild<gio::Menu>,
        pub settings: gio::Settings,
        pub store: glib::WeakRef<Store>,
        pub narrow: Cell<bool>,
        /// Set while the sidebar selection is being changed from code.
        pub syncing: Cell<bool>,
    }

    impl Default for Window {
        fn default() -> Self {
            Self {
                toast_overlay: Default::default(),
                stack: Default::default(),
                login_page: Default::default(),
                offline_status: Default::default(),
                split_view: Default::default(),
                household_label: Default::default(),
                household_button: Default::default(),
                nav_list: Default::default(),
                refresh_button: Default::default(),
                title_stack: Default::default(),
                page_title: Default::default(),
                list_switcher: Default::default(),
                list_switcher_label: Default::default(),
                offline_badge: Default::default(),
                last_refresh: Default::default(),
                last_refresh_time: Default::default(),
                pages: Default::default(),
                list_page: Default::default(),
                catalog_page: Default::default(),
                households_menu: Default::default(),
                households_submenu: Default::default(),
                lists_menu: Default::default(),
                settings: gio::Settings::new(APP_ID),
                store: Default::default(),
                narrow: Default::default(),
                syncing: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "OwletWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            LoginPage::ensure_type();
            ListPage::ensure_type();
            CatalogPage::ensure_type();
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[gtk::template_callbacks]
    impl Window {
        #[template_callback]
        fn on_narrow(&self) {
            self.narrow.set(true);
            self.list_page.set_narrow(true);
            self.login_page.set_narrow(true);
            self.obj().update_title();
        }

        #[template_callback]
        fn on_wide(&self) {
            self.narrow.set(false);
            self.list_page.set_narrow(false);
            self.login_page.set_narrow(false);
            self.obj().update_title();
        }

        #[template_callback]
        fn on_page_changed(&self) {
            let obj = self.obj();
            obj.update_title();
            obj.sync_sidebar();
        }

        #[template_callback]
        fn on_nav_row_activated(&self, row: &gtk::ListBoxRow) {
            self.obj().navigate(&row.widget_name());
        }
    }

    impl ObjectImpl for Window {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.load_window_size();
            obj.setup_actions();
            // The text moves from a time to "Yesterday" and on as the clock passes midnight.
            glib::timeout_add_seconds_local(
                60,
                glib::clone!(
                    #[weak]
                    obj,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        obj.update_last_refresh();
                        glib::ControlFlow::Continue
                    }
                ),
            );
            obj.connect_is_active_notify(|window| {
                if window.is_active()
                    && let Some(store) = window.store()
                {
                    store.refresh_if_stale();
                }
            });
        }
    }

    impl WidgetImpl for Window {}

    impl WindowImpl for Window {
        fn close_request(&self) -> glib::Propagation {
            self.obj().save_window_size();
            self.parent_close_request()
        }
    }

    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    pub fn new(app: &Application, store: &Store) -> Self {
        let window: Self = glib::Object::builder().property("application", app).build();
        window.set_store(store);
        window
    }

    pub fn toast(&self, text: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(text));
    }

    pub fn toast_with_undo(&self, text: &str, undo: impl Fn() + 'static) {
        let toast = adw::Toast::builder()
            .title(text)
            .button_label(gettext("_Undo"))
            .timeout(5)
            .build();
        toast.connect_button_clicked(move |_| undo());
        self.imp().toast_overlay.add_toast(toast);
    }

    pub fn store(&self) -> Option<Store> {
        self.imp().store.upgrade()
    }

    fn set_store(&self, store: &Store) {
        let imp = self.imp();
        imp.store.set(Some(store));
        imp.login_page.set_store(store);
        imp.list_page.set_store(store);
        imp.catalog_page.set_store(store);

        store.connect_busy_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |store| {
                let button = &window.imp().refresh_button;
                if store.busy() {
                    button.add_css_class("owlet-spinning");
                } else {
                    button.remove_css_class("owlet-spinning");
                }
                window.update_offline_badge();
                window.update_last_refresh();
            }
        ));
        for property in ["signed-in", "ready", "online"] {
            store.connect_notify_local(
                Some(property),
                glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |_, _| {
                        window.update_stack();
                        window.update_session_ui();
                    }
                ),
            );
        }
        store.connect_closure(
            "changed",
            false,
            glib::closure_local!(
                #[watch(rename_to = window)]
                self,
                move |_: Store| {
                    window.update_stack();
                    window.update_session_ui();
                }
            ),
        );
        store.connect_closure(
            "message",
            false,
            glib::closure_local!(
                #[watch(rename_to = window)]
                self,
                move |_: Store, text: String| window.toast(&text)
            ),
        );
        self.update_stack();
        self.update_session_ui();
    }

    fn setup_actions(&self) {
        let open_list = gio::ActionEntry::builder("open-list")
            .parameter_type(Some(glib::VariantTy::INT64))
            .activate(|window: &Self, _, param| {
                let Some(id) = param.and_then(|p| p.get::<i64>()) else {
                    return;
                };
                if let Some(store) = window.store() {
                    store.open_list(id);
                }
                window.imp().pages.set_visible_child_name("list");
            })
            .build();
        let household = gio::ActionEntry::builder("household")
            .parameter_type(Some(glib::VariantTy::INT64))
            .activate(|window: &Self, _, param| {
                let Some(id) = param.and_then(|p| p.get::<i64>()) else {
                    return;
                };
                if let Some(store) = window.store() {
                    glib::spawn_future_local(async move { store.switch_household(id).await });
                }
            })
            .build();
        let show_page = gio::ActionEntry::builder("show-page")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|window: &Self, _, param| {
                if let Some(name) = param.and_then(|p| p.get::<String>()) {
                    window.imp().pages.set_visible_child_name(&name);
                }
            })
            .build();
        let search = gio::ActionEntry::builder("search")
            .activate(|window: &Self, _, _| {
                let imp = window.imp();
                imp.pages.set_visible_child_name("catalog");
                imp.catalog_page.focus_search();
            })
            .build();
        let manage_lists = gio::ActionEntry::builder("manage-lists")
            .activate(|window: &Self, _, _| {
                if let Some(store) = window.store() {
                    manage_dialogs::show_list_manager(window, &store);
                }
            })
            .build();
        let manage_households = gio::ActionEntry::builder("manage-households")
            .activate(|window: &Self, _, _| {
                if let Some(store) = window.store() {
                    manage_dialogs::show_household_manager(window, &store);
                }
            })
            .build();
        let manage_categories = gio::ActionEntry::builder("manage-categories")
            .activate(|window: &Self, _, _| {
                if let Some(store) = window.store() {
                    manage_dialogs::show_category_manager(window, &store);
                }
            })
            .build();
        let new_item = gio::ActionEntry::builder("new-item")
            .activate(|window: &Self, _, _| {
                if let Some(store) = window.store() {
                    show_new_item(window, &store);
                }
            })
            .build();
        let add_item = gio::ActionEntry::builder("add-item")
            .activate(|window: &Self, _, _| {
                let imp = window.imp();
                imp.pages.set_visible_child_name("list");
                imp.list_page.focus_add();
            })
            .build();
        self.add_action_entries([
            open_list,
            household,
            show_page,
            search,
            manage_lists,
            manage_households,
            manage_categories,
            new_item,
            add_item,
        ]);
    }

    fn update_stack(&self) {
        let imp = self.imp();
        let Some(store) = self.store() else {
            return;
        };
        let page = if !store.ready() {
            "loading"
        } else if store.signed_in() && !store.online() && !store.has_data() {
            let host = store
                .client()
                .and_then(|c| c.base().host_str().map(str::to_owned))
                .unwrap_or_default();
            imp.offline_status
                .set_title(&gettext("Can't reach %s").replace("%s", &host));
            "offline"
        } else if store.signed_in() {
            "main"
        } else {
            imp.login_page.reset();
            "login"
        };
        imp.stack.set_visible_child_name(page);
    }

    /// Shown only once a refresh has settled on being offline, never while one is running.
    fn update_offline_badge(&self) {
        let imp = self.imp();
        let Some(store) = self.store() else {
            return;
        };
        let offline = !store.online() && !store.busy();
        imp.offline_badge.set_visible(offline);
        let (tooltip, label) = if offline {
            (gettext("Refresh (Offline)"), gettext("Refresh, offline"))
        } else {
            (gettext("Refresh"), gettext("Refresh"))
        };
        imp.refresh_button.set_tooltip_text(Some(&tooltip));
        imp.refresh_button
            .update_property(&[gtk::accessible::Property::Label(&label)]);
    }

    fn update_last_refresh(&self) {
        let imp = self.imp();
        let Some(store) = self.store() else {
            return;
        };
        let synced = Some(store.last_contact())
            .filter(|&ms| ms > 0 && !store.online() && !store.busy())
            .and_then(|ms| glib::DateTime::from_unix_local(ms / 1000).ok());
        let (Some(then), Ok(now)) = (synced, glib::DateTime::now_local()) else {
            imp.last_refresh.set_visible(false);
            return;
        };
        imp.last_refresh_time
            .set_label(&when::last_refresh(&then, &now));
        imp.last_refresh
            .set_tooltip_text(then.format("%c").ok().as_deref());
        imp.last_refresh.set_visible(true);
    }

    fn update_session_ui(&self) {
        let imp = self.imp();
        let Some(store) = self.store() else {
            return;
        };
        let online = store.online();
        self.update_offline_badge();
        self.update_last_refresh();
        for name in [
            "household",
            "manage-households",
            "manage-lists",
            "manage-categories",
            "new-item",
        ] {
            if let Some(action) = self.lookup_action(name).and_downcast::<gio::SimpleAction>() {
                action.set_enabled(online);
            }
        }
        imp.household_button.set_sensitive(online);

        let household = store.household();
        imp.household_label.set_label(
            &household
                .as_ref()
                .map_or_else(|| gettext("No Household"), |h| h.name.clone()),
        );

        let households = store.households();
        for menu in [&*imp.households_menu, &*imp.households_submenu] {
            menu.remove_all();
            if households.len() > 1 {
                let switch = gio::Menu::new();
                for h in &households {
                    let item = gio::MenuItem::new(Some(&h.name), None);
                    item.set_action_and_target_value(
                        Some("win.household"),
                        Some(&h.id.to_variant()),
                    );
                    switch.append_item(&item);
                }
                menu.append_section(None, &switch);
            }
            let manage = gio::Menu::new();
            manage.append(
                Some(&gettext("_Manage Households")),
                Some("win.manage-households"),
            );
            menu.append_section(None, &manage);
        }

        imp.lists_menu.remove_all();
        let lists_section = gio::Menu::new();
        for list in store.lists() {
            let item = gio::MenuItem::new(Some(&list.name), None);
            item.set_action_and_target_value(Some("win.open-list"), Some(&list.id.to_variant()));
            lists_section.append_item(&item);
        }
        imp.lists_menu.append_section(None, &lists_section);
        let manage = gio::Menu::new();
        manage.append(Some(&gettext("_Manage Lists")), Some("win.manage-lists"));
        imp.lists_menu.append_section(None, &manage);

        self.rebuild_sidebar();
        self.update_title();
    }

    fn rebuild_sidebar(&self) {
        let imp = self.imp();
        let Some(store) = self.store() else {
            return;
        };
        while let Some(row) = imp.nav_list.row_at_index(0) {
            imp.nav_list.remove(&row);
        }
        let lists_row = nav_row(
            "lists",
            Some("view-list-bullet-symbolic"),
            &gettext("Shopping Lists"),
            false,
        );
        if let Some(content) = lists_row.child().and_downcast::<gtk::Box>() {
            let manage = gtk::Button::from_icon_name("document-edit-symbolic");
            manage.add_css_class("flat");
            manage.set_tooltip_text(Some(&gettext("Manage Lists")));
            widgets::name(&manage, &gettext("Manage Lists"));
            manage.set_action_name(Some("win.manage-lists"));
            content.append(&manage);
        }
        imp.nav_list.append(&lists_row);
        for list in store.lists() {
            imp.nav_list.append(&nav_row(
                &format!("list:{}", list.id),
                None,
                &list.name,
                true,
            ));
        }
        let add = nav_row(
            "add-list",
            Some("list-add-symbolic"),
            &gettext("Add New List"),
            true,
        );
        add.add_css_class("dim-label");
        add.set_selectable(false);
        add.set_sensitive(store.online());
        imp.nav_list.append(&add);
        imp.nav_list.append(&nav_row(
            "catalog",
            Some("view-grid-symbolic"),
            &gettext("Item Catalog"),
            false,
        ));
        imp.nav_list.append(&nav_row(
            "recipes",
            Some("emoji-food-symbolic"),
            &gettext("Recipes"),
            false,
        ));
        imp.nav_list.append(&nav_row(
            "planner",
            Some("x-office-calendar-symbolic"),
            &gettext("Meal Planner"),
            false,
        ));
        self.sync_sidebar();
    }

    fn sync_sidebar(&self) {
        let imp = self.imp();
        let page = imp.pages.visible_child_name().unwrap_or_default();
        let wanted = if page == "list" {
            self.store()
                .and_then(|s| s.current_list())
                .map(|l| format!("list:{}", l.id))
                .unwrap_or_else(|| "lists".to_owned())
        } else {
            page.to_string()
        };
        let mut index = 0;
        while let Some(row) = imp.nav_list.row_at_index(index) {
            if row.widget_name() == wanted {
                imp.syncing.set(true);
                imp.nav_list.select_row(Some(&row));
                imp.syncing.set(false);
                break;
            }
            index += 1;
        }
    }

    fn navigate(&self, name: &str) {
        let imp = self.imp();
        let Some(store) = self.store() else {
            return;
        };
        match name {
            "profile" => {
                let _ = WidgetExt::activate_action(self, "app.profile", None);
            }
            "settings" => {
                let _ = WidgetExt::activate_action(self, "app.preferences", None);
            }
            "add-list" => {
                if let Some(row) = self.nav_row_named("add-list") {
                    manage_dialogs::ask_new_list(&row, &store);
                }
            }
            "lists" => {
                imp.pages.set_visible_child_name("list");
                self.sync_sidebar();
            }
            other => {
                if let Some(id) = other.strip_prefix("list:").and_then(|id| id.parse().ok()) {
                    store.open_list(id);
                    imp.pages.set_visible_child_name("list");
                } else {
                    imp.pages.set_visible_child_name(other);
                }
            }
        }
    }

    fn nav_row_named(&self, name: &str) -> Option<gtk::ListBoxRow> {
        let imp = self.imp();
        let mut index = 0;
        while let Some(row) = imp.nav_list.row_at_index(index) {
            if row.widget_name() == name {
                return Some(row);
            }
            index += 1;
        }
        None
    }

    fn update_title(&self) {
        let imp = self.imp();
        let page = imp.pages.visible_child_name().unwrap_or_default();
        let list_name = self
            .store()
            .and_then(|s| s.current_list())
            .map(|l| l.name)
            .unwrap_or_default();
        let title = match page.as_str() {
            "list" => list_name.clone(),
            "catalog" => gettext("Item Catalog"),
            "recipes" => gettext("Recipes"),
            "planner" => gettext("Meal Planner"),
            _ => String::new(),
        };
        imp.page_title.set_title(&title);
        imp.list_switcher_label.set_label(&list_name);
        let switcher = imp.narrow.get() && page == "list";
        imp.title_stack
            .set_visible_child_name(if switcher { "switcher" } else { "title" });
    }

    fn save_window_size(&self) {
        let settings = &self.imp().settings;
        let (width, height) = self.default_size();
        let _ = settings.set_int("window-width", width);
        let _ = settings.set_int("window-height", height);
        let _ = settings.set_boolean("is-maximized", self.is_maximized());
    }

    fn load_window_size(&self) {
        let settings = &self.imp().settings;
        self.set_default_size(settings.int("window-width"), settings.int("window-height"));
        if settings.boolean("is-maximized") {
            self.maximize();
        }
    }
}

fn nav_row(name: &str, icon: Option<&str>, label: &str, child: bool) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    if child {
        content.set_margin_start(28);
    }
    if let Some(icon) = icon {
        content.append(&gtk::Image::from_icon_name(icon));
    }
    let text = gtk::Label::builder()
        .label(label)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    content.append(&text);
    let row = gtk::ListBoxRow::builder().child(&content).build();
    row.set_widget_name(name);
    row
}

fn show_new_item(parent: &Window, store: &Store) {
    let categories = store.ordered_categories();
    let name_row = adw::EntryRow::builder().title(gettext("Name")).build();
    let mut labels = vec![gettext("None")];
    labels.extend(categories.iter().map(|c| c.name.clone()));
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let category_row = adw::ComboRow::builder()
        .title(gettext("Category"))
        .model(&gtk::StringList::new(&labels))
        .build();
    let group = adw::PreferencesGroup::new();
    group.add(&name_row);
    group.add(&category_row);

    let dialog = adw::AlertDialog::new(Some(&gettext("New Item")), None);
    dialog.set_extra_child(Some(&group));
    dialog.add_responses(&[
        ("cancel", &gettext("_Cancel")),
        ("create", &gettext("C_reate")),
    ]);
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("create"));
    dialog.set_close_response("cancel");
    dialog.set_response_enabled("create", false);
    name_row.connect_changed(glib::clone!(
        #[weak]
        dialog,
        move |row| dialog.set_response_enabled("create", !row.text().trim().is_empty())
    ));
    dialog.connect_response(
        Some("create"),
        glib::clone!(
            #[weak]
            store,
            move |_, _| {
                let name = name_row.text().trim().to_owned();
                let category = match category_row.selected() {
                    0 => None,
                    i => categories.get(i as usize - 1).map(|c| c.id),
                };
                glib::spawn_future_local(async move {
                    store.create_item(&name, category).await;
                });
            }
        ),
    );
    dialog.present(Some(parent));
}
