use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::api::{Category, Item, ListItem};
use crate::store::{SortOrder, Store};
use crate::ui::Window;
use crate::ui::chip_flow::ChipFlow;
use crate::ui::item_dialog::{self, OnList};
use crate::ui::item_icon;
use crate::ui::widgets;

const MAX_MATCHES: usize = 30;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ListPage {
        pub stack: gtk::Stack,
        pub groups: gtk::Box,
        pub recent_title: gtk::Label,
        pub recent: ChipFlow,
        pub matches: gtk::ListBox,
        pub top_slot: gtk::Box,
        pub bottom_slot: gtk::Box,
        pub add_bar: gtk::Box,
        pub entry: gtk::Entry,
        pub select_bar: gtk::ActionBar,
        pub select_label: gtk::Label,
        pub store: glib::WeakRef<Store>,
        /// Open list's ID, or 0 when there is none.
        pub list: Cell<i64>,
        /// Items marked in select-then-check mode.
        pub selected: RefCell<HashSet<i64>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ListPage {
        const NAME: &'static str = "OwletListPage";
        type Type = super::ListPage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for ListPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().build();
        }
    }

    impl WidgetImpl for ListPage {}
    impl BinImpl for ListPage {}
}

glib::wrapper! {
    pub struct ListPage(ObjectSubclass<imp::ListPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// Sort key of a category group: position and name. `None` is uncategorised.
type GroupKey = Option<(i64, String)>;

/// A titled run of items; `None` titles a list shown without category headers.
struct Group {
    title: Option<String>,
    items: Vec<ListItem>,
}

impl ListPage {
    fn build(&self) {
        let imp = self.imp();
        imp.groups.set_orientation(gtk::Orientation::Vertical);
        imp.groups.set_spacing(18);

        imp.recent_title.set_label(&gettext("Recently Bought"));
        imp.recent_title.set_xalign(0.0);
        imp.recent_title.add_css_class("heading");
        imp.recent.set_spacing(8);
        imp.recent.set_line_spacing(8);

        let recent_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        recent_box.append(&imp.recent_title);
        recent_box.append(&imp.recent);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
        content.append(&imp.groups);
        content.append(&recent_box);

        imp.matches.set_selection_mode(gtk::SelectionMode::None);
        imp.matches.add_css_class("boxed-list");
        imp.matches.set_valign(gtk::Align::Start);

        let empty = adw::StatusPage::builder()
            .icon_name("view-list-bullet-symbolic")
            .title(gettext("Nothing to Buy"))
            .description(gettext("Items added to this list show up here"))
            .build();

        imp.stack.add_named(&scrolled(&content), Some("items"));
        imp.stack
            .add_named(&scrolled(&imp.matches), Some("matches"));
        imp.stack.add_named(&empty, Some("empty"));
        imp.stack.set_vexpand(true);

        imp.entry
            .set_placeholder_text(Some(&gettext("Add an item, e.g. Milk, 2 L")));
        imp.entry.set_hexpand(true);
        imp.entry.set_primary_icon_name(Some("list-add-symbolic"));
        imp.entry
            .update_property(&[gtk::accessible::Property::Label(&gettext("Add an item"))]);
        imp.entry.connect_changed(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.show_matches()
        ));
        imp.entry.connect_activate(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |entry| {
                let text = entry.text().to_string();
                page.add_typed(&text);
            }
        ));
        let escape = gtk::EventControllerKey::new();
        escape.connect_key_pressed(glib::clone!(
            #[weak(rename_to = page)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape && !page.imp().entry.text().is_empty() {
                    page.imp().entry.set_text("");
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        ));
        imp.entry.add_controller(escape);

        imp.add_bar.set_spacing(8);
        imp.add_bar.append(&imp.entry);
        let clamp = adw::Clamp::builder()
            .maximum_size(720)
            .tightening_threshold(560)
            .child(&imp.add_bar)
            .build();
        clamp.set_margin_top(6);
        clamp.set_margin_bottom(6);
        clamp.set_margin_start(12);
        clamp.set_margin_end(12);
        imp.top_slot.append(&clamp);

        let clear = gtk::Button::with_mnemonic(&gettext("_Clear"));
        clear.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| {
                page.imp().selected.borrow_mut().clear();
                page.rebuild();
            }
        ));
        let check_off = gtk::Button::with_mnemonic(&gettext("Check _Off"));
        check_off.add_css_class("suggested-action");
        check_off.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.check_off_selected()
        ));
        imp.select_bar.pack_start(&clear);
        imp.select_bar.set_center_widget(Some(&imp.select_label));
        imp.select_bar.pack_end(&check_off);
        imp.select_bar.set_revealed(false);

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.append(&imp.top_slot);
        page.append(&imp.stack);
        page.append(&imp.select_bar);
        page.append(&imp.bottom_slot);
        self.set_child(Some(&page));
        self.add_css_class("owlet-list");
    }

    /// Puts the add bar within thumb reach on narrow windows.
    pub fn set_narrow(&self, narrow: bool) {
        let imp = self.imp();
        let Some(bar) = imp.add_bar.parent() else {
            return;
        };
        let (from, to) = if narrow {
            (&imp.top_slot, &imp.bottom_slot)
        } else {
            (&imp.bottom_slot, &imp.top_slot)
        };
        if bar.parent().as_ref() == Some(from.upcast_ref()) {
            from.remove(&bar);
            to.append(&bar);
        }
    }

    pub fn focus_add(&self) {
        self.imp().entry.grab_focus();
    }

    pub fn set_store(&self, store: &Store) {
        self.imp().store.set(Some(store));
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
        let settings = store.settings();
        for key in ["sort-order", "density", "tap-mode"] {
            settings.connect_changed(
                Some(key),
                glib::clone!(
                    #[weak(rename_to = page)]
                    self,
                    move |_, _| {
                        page.imp().selected.borrow_mut().clear();
                        page.rebuild();
                    }
                ),
            );
        }
        self.rebuild();
    }

    fn store(&self) -> Option<Store> {
        self.imp().store.upgrade()
    }

    pub fn rebuild(&self) {
        let imp = self.imp();
        let Some(store) = self.store() else {
            return;
        };
        let settings = store.settings();
        let compact = settings.string("density") == "compact";
        let select_mode = settings.string("tap-mode") == "select";
        if compact {
            self.add_css_class("compact");
        } else {
            self.remove_css_class("compact");
        }

        while let Some(child) = imp.groups.first_child() {
            imp.groups.remove(&child);
        }
        imp.recent.remove_all();

        let Some(list) = store.current_list() else {
            imp.list.set(0);
            imp.stack.set_visible_child_name("empty");
            self.update_select_bar();
            return;
        };
        if imp.list.replace(list.id) != list.id {
            imp.selected.borrow_mut().clear();
        }
        let present: HashSet<i64> = list.items.iter().map(|i| i.item.id).collect();
        imp.selected.borrow_mut().retain(|id| present.contains(id));

        let sort = SortOrder::from_settings(&settings);
        let icons = list
            .items
            .iter()
            .any(|i| item_icon::has(i.item.icon.as_deref()));
        for group in group_items(list.items.clone(), &store.categories(), sort) {
            imp.groups
                .append(&self.group_widget(&group, compact, select_mode, icons));
        }

        let mut shown = 0;
        for recent in list
            .recent_items
            .iter()
            .filter(|r| !present.contains(&r.item.id))
        {
            imp.recent.append(&self.recent_chip(recent));
            shown += 1;
        }
        imp.recent_title.set_visible(shown > 0);
        imp.recent.set_visible(shown > 0);

        self.show_matches();
        self.update_select_bar();
    }

    fn update_select_bar(&self) {
        let imp = self.imp();
        let count = imp.selected.borrow().len();
        imp.select_bar.set_revealed(count > 0);
        imp.select_label.set_label(
            &gettextrs::ngettext("%d selected", "%d selected", count as u32)
                .replace("%d", &count.to_string()),
        );
    }

    fn group_widget(
        &self,
        group: &Group,
        compact: bool,
        select_mode: bool,
        icons: bool,
    ) -> gtk::Widget {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        for item in &group.items {
            list.append(&self.item_row(item, compact, select_mode, icons));
        }
        let Some(title) = &group.title else {
            return list.upcast();
        };
        let label = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .margin_start(6)
            .wrap(true)
            .build();
        label.add_css_class("heading");
        label.add_css_class("dim-label");
        label.add_css_class("owlet-category");
        let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
        section.append(&label);
        section.append(&list);
        section.upcast()
    }

    fn item_row(
        &self,
        entry: &ListItem,
        compact: bool,
        select_mode: bool,
        icons: bool,
    ) -> gtk::ListBoxRow {
        let imp = self.imp();
        let check = gtk::CheckButton::new();
        check.add_css_class("selection-mode");
        check.set_valign(gtk::Align::Center);
        check.update_property(&[gtk::accessible::Property::Label(
            &gettext("Check off %s").replace("%s", &entry.item.name),
        )]);
        let selected = select_mode && imp.selected.borrow().contains(&entry.item.id);
        check.set_active(selected);

        let icon = gtk::Image::builder()
            .pixel_size(if compact { 20 } else { 24 })
            .valign(gtk::Align::Center)
            .build();
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);
        item_icon::show(&icon, entry.item.icon.as_deref(), icons);

        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        let name = gtk::Label::builder().xalign(0.0).build();
        name.add_css_class("owlet-item-name");
        if selected {
            name.add_css_class("owlet-marked");
        }
        if compact {
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            if entry.description.is_empty() {
                name.set_label(&entry.item.name);
            } else {
                name.set_markup(&format!(
                    "{} <span alpha=\"60%\">· {}</span>",
                    glib::markup_escape_text(&entry.item.name),
                    glib::markup_escape_text(&entry.description)
                ));
            }
            text.append(&name);
        } else {
            name.set_label(&entry.item.name);
            name.set_wrap(true);
            name.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            text.append(&name);
            if !entry.description.is_empty() {
                let description = gtk::Label::builder()
                    .label(&entry.description)
                    .xalign(0.0)
                    .wrap(true)
                    .build();
                description.add_css_class("dim-label");
                description.add_css_class("caption");
                text.append(&description);
            }
        }

        let store = self.store();
        if store
            .as_ref()
            .is_some_and(|s| s.is_waiting(imp.list.get(), entry.item.id))
        {
            text.add_css_class("owlet-pending");
            text.update_property(&[gtk::accessible::Property::Description(&gettext(
                "Waiting to sync",
            ))]);
        }

        let edit = gtk::Button::from_icon_name("document-edit-symbolic");
        edit.add_css_class("flat");
        edit.set_valign(gtk::Align::Center);
        edit.set_tooltip_text(Some(&gettext("Edit Item")));
        widgets::name(&edit, &gettext("Edit “%s”").replace("%s", &entry.item.name));
        edit.set_sensitive(store.as_ref().is_some_and(Store::online));
        let item = entry.item.clone();
        let description = entry.description.clone();
        edit.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.edit(&item, &description)
        ));

        let entry = entry.clone();
        check.connect_toggled(glib::clone!(
            #[weak(rename_to = page)]
            self,
            #[weak]
            name,
            move |check| page.on_check_toggled(check, &name, &entry)
        ));

        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row_box.add_css_class("owlet-item");
        row_box.append(&check);
        row_box.append(&icon);
        row_box.append(&text);
        row_box.append(&edit);

        gtk::ListBoxRow::builder()
            .activatable(false)
            .child(&row_box)
            .build()
    }

    fn on_check_toggled(&self, check: &gtk::CheckButton, name: &gtk::Label, entry: &ListItem) {
        let imp = self.imp();
        let Some(store) = self.store() else {
            return;
        };
        let list = imp.list.get();
        if store.settings().string("tap-mode") == "select" {
            let id = entry.item.id;
            if check.is_active() {
                imp.selected.borrow_mut().insert(id);
                name.add_css_class("owlet-marked");
            } else {
                imp.selected.borrow_mut().remove(&id);
                name.remove_css_class("owlet-marked");
            }
            self.update_select_bar();
            return;
        }
        if !check.is_active() {
            return;
        }
        check.set_sensitive(false);
        let entry = entry.clone();
        let window = self.root().and_downcast::<Window>();
        glib::spawn_future_local(async move {
            if store.check_off(list, &[entry.item.id]).await
                && let Some(window) = window
            {
                let name = entry.item.name.clone();
                let description = entry.description.clone();
                window.toast_with_undo(
                    &gettext("“%s” checked off").replace("%s", &name),
                    glib::clone!(
                        #[weak]
                        store,
                        move || {
                            let name = name.clone();
                            let description = description.clone();
                            glib::spawn_future_local(async move {
                                store.add_to_list(list, &name, Some(&description)).await;
                            });
                        }
                    ),
                );
            }
        });
    }

    fn check_off_selected(&self) {
        let imp = self.imp();
        let Some(store) = self.store() else {
            return;
        };
        let ids: Vec<i64> = imp.selected.borrow_mut().drain().collect();
        if ids.is_empty() {
            return;
        }
        let list = imp.list.get();
        self.update_select_bar();
        glib::spawn_future_local(async move {
            store.check_off(list, &ids).await;
        });
    }

    fn edit(&self, item: &Item, description: &str) {
        let Some(store) = self.store() else {
            return;
        };
        let on_list = OnList {
            list: self.imp().list.get(),
            description: description.to_owned(),
        };
        item_dialog::show_item_editor(self, &store, item, Some(on_list));
    }

    fn recent_chip(&self, entry: &ListItem) -> gtk::Button {
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        content.append(&gtk::Image::from_icon_name("list-add-symbolic"));
        content.append(
            &gtk::Label::builder()
                .label(&entry.item.name)
                .xalign(0.0)
                .build(),
        );
        let button = gtk::Button::builder()
            .child(&content)
            .halign(gtk::Align::Start)
            .build();
        button.add_css_class("owlet-chip");
        button.set_tooltip_text(Some(&gettext("Add Again")));
        widgets::name(
            &button,
            &gettext("Add “%s” Again").replace("%s", &entry.item.name),
        );
        let name = entry.item.name.clone();
        let description = entry.description.clone();
        button.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |button| {
                button.set_sensitive(false);
                page.add(&name, Some(&description));
            }
        ));
        button
    }

    fn add(&self, name: &str, description: Option<&str>) {
        let Some(store) = self.store() else {
            return;
        };
        let list = self.imp().list.get();
        if list == 0 {
            return;
        }
        let name = name.to_owned();
        let description = description.map(str::to_owned);
        glib::spawn_future_local(async move {
            store.add_to_list(list, &name, description.as_deref()).await;
        });
    }

    /// Text after the first comma becomes the description, as in KitchenOwl.
    fn add_typed(&self, text: &str) {
        let (name, description) = match text.split_once(',') {
            Some((name, description)) => (name.trim(), Some(description.trim())),
            None => (text.trim(), None),
        };
        if name.is_empty() {
            return;
        }
        if let Some(store) = self.store()
            && !store.can_add(name)
        {
            store.show_message(&gettext("This change needs a connection"));
            return;
        }
        self.add(name, description);
        self.imp().entry.set_text("");
    }

    fn show_matches(&self) {
        let imp = self.imp();
        let typed = imp.entry.text().trim().to_owned();
        if typed.is_empty() {
            let has_content = imp.groups.first_child().is_some() || imp.recent.is_visible();
            imp.stack
                .set_visible_child_name(if has_content { "items" } else { "empty" });
            return;
        }
        let Some(store) = self.store() else {
            return;
        };
        let on_list: HashSet<i64> = store
            .current_list()
            .map(|l| l.items.iter().map(|i| i.item.id).collect())
            .unwrap_or_default();
        let whole = typed.to_lowercase();
        let name_part = whole
            .split(',')
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let mut items: Vec<Item> = store
            .items()
            .into_iter()
            .filter(|i| {
                let lower = i.name.to_lowercase();
                lower.contains(&whole) || lower.contains(&name_part)
            })
            .collect();
        items.sort_by_key(|i| {
            let lower = i.name.to_lowercase();
            (!lower.starts_with(&name_part), !i.is_used(), lower)
        });
        while let Some(row) = imp.matches.row_at_index(0) {
            imp.matches.remove(&row);
        }
        let exact = items
            .iter()
            .any(|i| i.name.to_lowercase() == whole || i.name.to_lowercase() == name_part);
        if !exact {
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&typed))
                .subtitle(gettext("Add as typed"))
                .activatable(true)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name("list-add-symbolic"));
            row.connect_activated(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |_| page.add_typed(&typed)
            ));
            imp.matches.append(&row);
        }
        for item in items.into_iter().take(MAX_MATCHES) {
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&item.name))
                .activatable(true)
                .build();
            if on_list.contains(&item.id) {
                row.set_subtitle(&gettext("Already on this list"));
                row.set_sensitive(false);
            }
            let name = item.name.clone();
            row.connect_activated(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |_| {
                    page.add(&name, None);
                    page.imp().entry.set_text("");
                    page.imp().entry.grab_focus();
                }
            ));
            imp.matches.append(&row);
        }
        imp.stack.set_visible_child_name("matches");
    }
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let clamp = adw::Clamp::builder()
        .maximum_size(720)
        .tightening_threshold(560)
        .child(child)
        .build();
    clamp.set_margin_top(6);
    clamp.set_margin_bottom(24);
    clamp.set_margin_start(12);
    clamp.set_margin_end(12);
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .build()
}

fn by_name(a: &ListItem, b: &ListItem) -> Ordering {
    a.item.name.to_lowercase().cmp(&b.item.name.to_lowercase())
}

fn group_items(mut items: Vec<ListItem>, categories: &[Category], sort: SortOrder) -> Vec<Group> {
    match sort {
        SortOrder::Algorithmic => vec![Group { title: None, items }],
        SortOrder::Alphabetical => {
            items.sort_by(by_name);
            vec![Group { title: None, items }]
        }
        SortOrder::Category => {
            // The categories endpoint has the current order; an item's embedded copy can be stale.
            let known: HashMap<i64, &Category> = categories.iter().map(|c| (c.id, c)).collect();
            let mut by_category: Vec<(GroupKey, String, Vec<ListItem>)> = Vec::new();
            for item in items {
                let (key, title) = match &item.item.category {
                    Some(c) => {
                        let current = known.get(&c.id).copied().unwrap_or(c);
                        (
                            Some((current.ordering, current.name.clone())),
                            current.name.clone(),
                        )
                    }
                    None => (None, gettext("Other")),
                };
                match by_category.iter_mut().find(|(k, _, _)| *k == key) {
                    Some((_, _, group)) => group.push(item),
                    None => by_category.push((key, title, vec![item])),
                }
            }
            // Uncategorised items go last.
            by_category.sort_by(|(a, _, _), (b, _, _)| match (a, b) {
                (Some(a), Some(b)) => a.cmp(b),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            });
            by_category
                .into_iter()
                .map(|(_, title, mut items)| {
                    items.sort_by(by_name);
                    Group {
                        title: Some(title),
                        items,
                    }
                })
                .collect()
        }
    }
}
