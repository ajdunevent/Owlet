use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::api::Item;
use crate::store::{SortOrder, Store};
use crate::ui::{item_dialog, widgets};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct CatalogPage {
        pub search: gtk::SearchEntry,
        pub show_all: gtk::Switch,
        pub stack: gtk::Stack,
        pub groups: gtk::Box,
        pub store: glib::WeakRef<Store>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CatalogPage {
        const NAME: &'static str = "OwletCatalogPage";
        type Type = super::CatalogPage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for CatalogPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().build();
        }
    }

    impl WidgetImpl for CatalogPage {}
    impl BinImpl for CatalogPage {}
}

glib::wrapper! {
    pub struct CatalogPage(ObjectSubclass<imp::CatalogPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CatalogPage {
    fn build(&self) {
        let imp = self.imp();
        imp.search
            .set_placeholder_text(Some(&gettext("Search items")));
        imp.search.set_hexpand(true);
        imp.search.connect_search_changed(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.rebuild()
        ));

        let show_all_label = gtk::Label::new(Some(&gettext("Show all items")));
        show_all_label.set_mnemonic_widget(Some(&imp.show_all));
        imp.show_all.set_valign(gtk::Align::Center);
        let show_all_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        show_all_box.append(&show_all_label);
        show_all_box.append(&imp.show_all);

        let new_item = gtk::Button::from_icon_name("list-add-symbolic");
        new_item.set_tooltip_text(Some(&gettext("New Item")));
        widgets::name(&new_item, &gettext("New Item"));
        new_item.set_action_name(Some("win.new-item"));
        let menu = gio::Menu::new();
        menu.append(
            Some(&gettext("_Manage Categories")),
            Some("win.manage-categories"),
        );
        let more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text(gettext("More"))
            .menu_model(&menu)
            .build();
        widgets::name(&more, &gettext("More"));

        let search_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        search_row.append(&imp.search);
        search_row.append(&new_item);
        search_row.append(&more);
        show_all_box.set_halign(gtk::Align::End);
        let controls = gtk::Box::new(gtk::Orientation::Vertical, 6);
        controls.append(&search_row);
        controls.append(&show_all_box);

        imp.groups.set_orientation(gtk::Orientation::Vertical);
        imp.groups.set_spacing(18);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(6);
        content.set_margin_bottom(24);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.append(&imp.groups);

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(
                &adw::Clamp::builder()
                    .maximum_size(720)
                    .tightening_threshold(560)
                    .child(&content)
                    .build(),
            )
            .build();

        let empty = adw::StatusPage::builder()
            .icon_name("edit-find-symbolic")
            .title(gettext("No Matching Items"))
            .build();
        imp.stack.add_named(&scrolled, Some("items"));
        imp.stack.add_named(&empty, Some("empty"));
        imp.stack.set_vexpand(true);

        let top = adw::Clamp::builder()
            .maximum_size(720)
            .tightening_threshold(560)
            .child(&controls)
            .build();
        top.set_margin_top(6);
        top.set_margin_bottom(6);
        top.set_margin_start(12);
        top.set_margin_end(12);

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.append(&top);
        page.append(&imp.stack);
        self.set_child(Some(&page));
    }

    pub fn set_store(&self, store: &Store) {
        let imp = self.imp();
        imp.store.set(Some(store));
        let settings = store.settings();
        settings
            .bind("catalog-show-all", &imp.show_all, "active")
            .build();
        store.connect_closure(
            "changed",
            false,
            glib::closure_local!(
                #[watch(rename_to = page)]
                self,
                move |_: Store| page.rebuild()
            ),
        );
        store.connect_online_notify(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.rebuild()
        ));
        for key in ["catalog-show-all", "sort-order"] {
            settings.connect_changed(
                Some(key),
                glib::clone!(
                    #[weak(rename_to = page)]
                    self,
                    move |_, _| page.rebuild()
                ),
            );
        }
        self.rebuild();
    }

    pub fn focus_search(&self) {
        self.imp().search.grab_focus();
    }

    pub fn rebuild(&self) {
        let imp = self.imp();
        let Some(store) = imp.store.upgrade() else {
            return;
        };
        while let Some(child) = imp.groups.first_child() {
            imp.groups.remove(&child);
        }
        let settings = store.settings();
        let query = imp.search.text().to_lowercase();
        let show_all = settings.boolean("catalog-show-all") || !query.is_empty();
        let by_category = SortOrder::from_settings(&settings) == SortOrder::Category;
        let categories = store.categories();
        let category_key = |item: &Item| {
            item.category.as_ref().map(|c| {
                categories
                    .iter()
                    .find(|known| known.id == c.id)
                    .map_or((c.ordering, c.name.clone()), |k| {
                        (k.ordering, k.name.clone())
                    })
            })
        };

        let mut items: Vec<Item> = store
            .items()
            .into_iter()
            .filter(|i| show_all || i.is_used())
            .filter(|i| query.is_empty() || i.name.to_lowercase().contains(&query))
            .collect();
        items.sort_by(|a, b| {
            let name = a.name.to_lowercase().cmp(&b.name.to_lowercase());
            if !by_category {
                return name;
            }
            // Uncategorised items go last.
            match (category_key(a), category_key(b)) {
                (Some(x), Some(y)) => x.cmp(&y).then(name),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => name,
            }
        });

        let mut current: Option<(Option<(i64, String)>, gtk::ListBox)> = None;
        for item in &items {
            let key = if by_category {
                category_key(item)
            } else {
                None
            };
            if current.as_ref().is_none_or(|(k, _)| *k != key) {
                let list = gtk::ListBox::new();
                list.set_selection_mode(gtk::SelectionMode::None);
                list.add_css_class("boxed-list");
                let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
                if by_category {
                    let title = key
                        .as_ref()
                        .map_or_else(|| gettext("Other"), |(_, name)| name.clone());
                    let header = gtk::Label::builder()
                        .label(&title)
                        .xalign(0.0)
                        .margin_start(6)
                        .wrap(true)
                        .build();
                    header.add_css_class("heading");
                    header.add_css_class("dim-label");
                    section.append(&header);
                }
                section.append(&list);
                imp.groups.append(&section);
                current = Some((key.clone(), list));
            }
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&item.name))
                .activatable(store.online())
                .build();
            row.add_suffix(&self.add_to_list_button(item, &store));
            let edited = item.clone();
            row.connect_activated(glib::clone!(
                #[weak(rename_to = page)]
                self,
                #[weak]
                store,
                move |_| item_dialog::show_item_editor(&page, &store, &edited, None)
            ));
            if !by_category && let Some((_, name)) = category_key(item) {
                row.set_subtitle(&glib::markup_escape_text(&name));
            }
            if let Some((_, list)) = &current {
                list.append(&row);
            }
        }
        imp.stack
            .set_visible_child_name(if items.is_empty() { "empty" } else { "items" });
    }

    /// Adds straight to a household's only list; otherwise asks which list, greying out lists
    /// the item is already on.
    fn add_to_list_button(&self, item: &Item, store: &Store) -> gtk::Widget {
        let lists = store.lists();
        let on: Vec<bool> = lists
            .iter()
            .map(|l| l.items.iter().any(|i| i.item.id == item.id))
            .collect();
        let tooltip = gettext("Add to List");
        if let [list] = lists.as_slice() {
            let button = gtk::Button::from_icon_name("list-add-symbolic");
            button.add_css_class("flat");
            button.set_valign(gtk::Align::Center);
            button.set_tooltip_text(Some(&tooltip));
            widgets::name(
                &button,
                &gettext("Add “%s” to %s")
                    .replacen("%s", &item.name, 1)
                    .replacen("%s", &list.name, 1),
            );
            button.set_sensitive(!on[0]);
            let (list_id, name) = (list.id, item.name.clone());
            button.connect_clicked(glib::clone!(
                #[weak]
                store,
                move |button| {
                    button.set_sensitive(false);
                    let name = name.clone();
                    glib::spawn_future_local(async move {
                        store.add_to_list(list_id, &name, None).await;
                    });
                }
            ));
            return button.upcast();
        }
        let popover = gtk::Popover::new();
        popover.add_css_class("menu");
        let menu = gtk::Box::new(gtk::Orientation::Vertical, 0);
        for (list, already) in lists.iter().zip(&on) {
            let label = if *already {
                gettext("%s (already on it)").replace("%s", &list.name)
            } else {
                list.name.clone()
            };
            let entry = gtk::Button::builder()
                .child(&gtk::Label::builder().label(&label).xalign(0.0).build())
                .sensitive(!already)
                .build();
            entry.add_css_class("flat");
            let (list_id, name) = (list.id, item.name.clone());
            entry.connect_clicked(glib::clone!(
                #[weak]
                store,
                #[weak]
                popover,
                move |_| {
                    popover.popdown();
                    let name = name.clone();
                    glib::spawn_future_local(async move {
                        store.add_to_list(list_id, &name, None).await;
                    });
                }
            ));
            menu.append(&entry);
        }
        popover.set_child(Some(&menu));
        let button = gtk::MenuButton::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text(&tooltip)
            .valign(gtk::Align::Center)
            .popover(&popover)
            .sensitive(on.iter().any(|already| !already))
            .build();
        widgets::name(
            &button,
            &gettext("Add “%s” to a List").replace("%s", &item.name),
        );
        button.add_css_class("flat");
        button.upcast()
    }
}
