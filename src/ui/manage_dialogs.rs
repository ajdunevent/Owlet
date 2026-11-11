use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::api::CategoryUpdate;
use crate::store::Store;
use crate::ui::widgets;

fn sheet(title: &str, list: &gtk::ListBox) -> adw::Dialog {
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    list.set_valign(gtk::Align::Start);
    let content = adw::Clamp::builder().maximum_size(480).child(list).build();
    content.set_margin_top(6);
    content.set_margin_bottom(24);
    content.set_margin_start(12);
    content.set_margin_end(12);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&content)
            .build(),
    ));
    adw::Dialog::builder()
        .title(title)
        .content_width(420)
        .child(&view)
        .build()
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some(tooltip));
    widgets::name(&button, tooltip);
    button
}

/// Rebuilds `fill` into `list` now and whenever the store changes, while `dialog` is open.
fn keep_filled(
    dialog: &adw::Dialog,
    store: &Store,
    list: &gtk::ListBox,
    fill: impl Fn(&gtk::ListBox) + 'static,
) {
    fill(list);
    let handler = store.connect_closure(
        "changed",
        false,
        glib::closure_local!(
            #[weak]
            list,
            #[weak]
            dialog,
            move |_: Store| {
                fill(&list);
                fit_height(&dialog);
            }
        ),
    );
    let handler = std::cell::Cell::new(Some(handler));
    dialog.connect_closed(glib::clone!(
        #[weak]
        store,
        move |_| {
            if let Some(handler) = handler.take() {
                store.disconnect(handler);
            }
        }
    ));
}

/// A dialog keeps the height it opened with, so it is re-measured when rows come or go.
fn fit_height(dialog: &adw::Dialog) {
    if let Some(child) = dialog.child() {
        let (_, natural, _, _) = child.measure(gtk::Orientation::Vertical, dialog.content_width());
        dialog.set_content_height(natural);
    }
}

fn add_row(label: &str) -> adw::ButtonRow {
    adw::ButtonRow::builder()
        .title(label)
        .use_underline(true)
        .start_icon_name("list-add-symbolic")
        .build()
}

/// Shared by the narrow layout's Manage Lists sheet and the wide sidebar.
pub fn ask_new_list(anchor: &impl IsA<gtk::Widget>, store: &Store) {
    widgets::name_popover(
        anchor,
        "",
        &gettext("_Add"),
        glib::clone!(
            #[weak]
            store,
            move |name| {
                glib::spawn_future_local(async move {
                    store.add_list(&name).await;
                });
            }
        ),
    );
}

fn ask_rename_list(anchor: &impl IsA<gtk::Widget>, store: &Store, list: i64, current: &str) {
    widgets::name_popover(
        anchor,
        current,
        &gettext("_Rename"),
        glib::clone!(
            #[weak]
            store,
            move |name| {
                glib::spawn_future_local(async move {
                    store.rename_list(list, &name).await;
                });
            }
        ),
    );
}

fn ask_delete_list(parent: &impl IsA<gtk::Widget>, store: &Store, list: i64) {
    let Some(target) = store.lists().into_iter().find(|l| l.id == list) else {
        return;
    };
    let count = target.items.len();
    let body = if count == 0 {
        gettext("The list is empty.")
    } else {
        gettext("Its %d items will be removed.").replace("%d", &count.to_string())
    };
    widgets::confirm(
        parent,
        &gettext("Delete “%s”?").replace("%s", &target.name),
        &body,
        &gettext("_Delete"),
        glib::clone!(
            #[weak]
            store,
            move || {
                glib::spawn_future_local(async move {
                    store.delete_list(list).await;
                });
            }
        ),
    );
}

pub fn show_list_manager(parent: &impl IsA<gtk::Widget>, store: &Store) {
    let list = gtk::ListBox::new();
    let dialog = sheet(&gettext("Lists"), &list);
    keep_filled(
        &dialog,
        store,
        &list,
        glib::clone!(
            #[weak]
            store,
            #[weak]
            dialog,
            move |list| {
                while let Some(row) = list.row_at_index(0) {
                    list.remove(&row);
                }
                let default = store.default_list_id();
                for shopping_list in store.lists() {
                    let row = adw::ActionRow::builder()
                        .title(glib::markup_escape_text(&shopping_list.name))
                        .build();
                    let rename = icon_button("document-edit-symbolic", &gettext("Rename"));
                    let id = shopping_list.id;
                    let name = shopping_list.name.clone();
                    rename.connect_clicked(glib::clone!(
                        #[weak]
                        store,
                        move |button| ask_rename_list(button, &store, id, &name)
                    ));
                    row.add_suffix(&rename);
                    if Some(id) != default {
                        let delete = icon_button("window-close-symbolic", &gettext("Delete"));
                        delete.connect_clicked(glib::clone!(
                            #[weak]
                            store,
                            #[weak]
                            dialog,
                            move |_| ask_delete_list(&dialog, &store, id)
                        ));
                        row.add_suffix(&delete);
                    }
                    list.append(&row);
                }
                let add = add_row(&gettext("_Add New List"));
                add.connect_activated(glib::clone!(
                    #[weak]
                    store,
                    move |row| ask_new_list(row, &store)
                ));
                list.append(&add);
            }
        ),
    );
    dialog.present(Some(parent));
}

pub fn show_household_manager(parent: &impl IsA<gtk::Widget>, store: &Store) {
    let list = gtk::ListBox::new();
    let dialog = sheet(&gettext("Households"), &list);
    keep_filled(
        &dialog,
        store,
        &list,
        glib::clone!(
            #[weak]
            store,
            #[weak]
            dialog,
            move |list| {
                while let Some(row) = list.row_at_index(0) {
                    list.remove(&row);
                }
                for household in store.households() {
                    let row = adw::ActionRow::builder()
                        .title(glib::markup_escape_text(&household.name))
                        .build();
                    let id = household.id;
                    let name = household.name.clone();
                    if store.can_manage_household(&household) {
                        let rename = icon_button("document-edit-symbolic", &gettext("Rename"));
                        let current = name.clone();
                        rename.connect_clicked(glib::clone!(
                            #[weak]
                            store,
                            move |button| ask_rename_household(button, &store, id, &current)
                        ));
                        row.add_suffix(&rename);
                        let delete = icon_button("window-close-symbolic", &gettext("Delete"));
                        delete.connect_clicked(glib::clone!(
                            #[weak]
                            store,
                            #[weak]
                            dialog,
                            move |_| ask_delete_household(&dialog, &store, id, &name)
                        ));
                        row.add_suffix(&delete);
                    } else {
                        let leave = icon_button("system-log-out-symbolic", &gettext("Leave"));
                        leave.connect_clicked(glib::clone!(
                            #[weak]
                            store,
                            #[weak]
                            dialog,
                            move |_| ask_leave_household(&dialog, &store, id, &name)
                        ));
                        row.add_suffix(&leave);
                    }
                    list.append(&row);
                }
                let add = add_row(&gettext("_Add New Household"));
                add.connect_activated(glib::clone!(
                    #[weak]
                    store,
                    move |row| ask_new_household(row, &store)
                ));
                list.append(&add);
            }
        ),
    );
    dialog.present(Some(parent));
}

fn ask_new_household(anchor: &impl IsA<gtk::Widget>, store: &Store) {
    widgets::name_popover(
        anchor,
        "",
        &gettext("_Add"),
        glib::clone!(
            #[weak]
            store,
            move |name| {
                glib::spawn_future_local(async move {
                    store.add_household(&name).await;
                });
            }
        ),
    );
}

fn ask_rename_household(anchor: &impl IsA<gtk::Widget>, store: &Store, id: i64, current: &str) {
    widgets::name_popover(
        anchor,
        current,
        &gettext("_Rename"),
        glib::clone!(
            #[weak]
            store,
            move |name| {
                glib::spawn_future_local(async move {
                    store.rename_household(id, &name).await;
                });
            }
        ),
    );
}

fn ask_delete_household(parent: &impl IsA<gtk::Widget>, store: &Store, id: i64, name: &str) {
    widgets::confirm(
        parent,
        &gettext("Delete “%s”?").replace("%s", name),
        &gettext("Its lists, items and categories are removed for every member."),
        &gettext("_Delete"),
        glib::clone!(
            #[weak]
            store,
            move || {
                glib::spawn_future_local(async move {
                    store.delete_household(id).await;
                });
            }
        ),
    );
}

fn ask_leave_household(parent: &impl IsA<gtk::Widget>, store: &Store, id: i64, name: &str) {
    widgets::confirm(
        parent,
        &gettext("Leave “%s”?").replace("%s", name),
        &gettext("A member will need to add you again."),
        &gettext("_Leave"),
        glib::clone!(
            #[weak]
            store,
            move || {
                glib::spawn_future_local(async move {
                    store.leave_household(id).await;
                });
            }
        ),
    );
}

pub fn show_category_manager(parent: &impl IsA<gtk::Widget>, store: &Store) {
    let list = gtk::ListBox::new();
    let dialog = sheet(&gettext("Categories"), &list);
    keep_filled(
        &dialog,
        store,
        &list,
        glib::clone!(
            #[weak]
            store,
            #[weak]
            dialog,
            move |list| {
                while let Some(row) = list.row_at_index(0) {
                    list.remove(&row);
                }
                let categories = store.ordered_categories();
                let last = categories.len().saturating_sub(1);
                for (index, category) in categories.iter().enumerate() {
                    let row = adw::ActionRow::builder()
                        .title(glib::markup_escape_text(&category.name))
                        .build();
                    let id = category.id;
                    let up = icon_button("go-up-symbolic", &gettext("Move Up"));
                    up.set_sensitive(index > 0);
                    up.connect_clicked(glib::clone!(
                        #[weak]
                        store,
                        move |_| move_category(&store, id, index as i64 - 1)
                    ));
                    let down = icon_button("go-down-symbolic", &gettext("Move Down"));
                    down.set_sensitive(index < last);
                    down.connect_clicked(glib::clone!(
                        #[weak]
                        store,
                        move |_| move_category(&store, id, index as i64 + 1)
                    ));
                    row.add_suffix(&up);
                    row.add_suffix(&down);
                    row.add_suffix(&category_menu(&dialog, &store, id, &category.name));
                    list.append(&row);
                }
                let add = add_row(&gettext("_Add New Category"));
                add.connect_activated(glib::clone!(
                    #[weak]
                    store,
                    move |row| {
                        widgets::name_popover(
                            row,
                            "",
                            &gettext("_Add"),
                            glib::clone!(
                                #[weak]
                                store,
                                move |name| {
                                    glib::spawn_future_local(async move {
                                        store.add_category(&name).await;
                                    });
                                }
                            ),
                        );
                    }
                ));
                list.append(&add);
            }
        ),
    );
    dialog.present(Some(parent));
}

fn move_category(store: &Store, id: i64, position: i64) {
    let store = store.clone();
    let update = CategoryUpdate {
        ordering: Some(position),
        ..Default::default()
    };
    glib::spawn_future_local(async move {
        store.update_category(id, update).await;
    });
}

fn category_menu(dialog: &adw::Dialog, store: &Store, id: i64, name: &str) -> gtk::MenuButton {
    let popover = gtk::Popover::new();
    let menu = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let entries = [
        ("rename", gettext("_Rename")),
        ("merge", gettext("_Merge Into…")),
        ("delete", gettext("_Delete")),
    ];
    let button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(gettext("More"))
        .valign(gtk::Align::Center)
        .popover(&popover)
        .build();
    widgets::name(&button, &gettext("More"));
    button.add_css_class("flat");
    for (action, label) in entries {
        let item = gtk::Button::builder()
            .child(
                &gtk::Label::builder()
                    .label(&label)
                    .use_underline(true)
                    .xalign(0.0)
                    .build(),
            )
            .build();
        item.add_css_class("flat");
        let name = name.to_owned();
        item.connect_clicked(glib::clone!(
            #[weak]
            popover,
            #[weak]
            button,
            #[weak]
            dialog,
            #[weak]
            store,
            move |_| {
                popover.popdown();
                match action {
                    "rename" => rename_category(&button, &store, id, &name),
                    "merge" => merge_category(&dialog, &store, id, &name),
                    _ => delete_category(&dialog, &store, id, &name),
                }
            }
        ));
        menu.append(&item);
    }
    popover.set_child(Some(&menu));
    popover.add_css_class("menu");
    button
}

fn rename_category(anchor: &gtk::MenuButton, store: &Store, id: i64, name: &str) {
    widgets::name_popover(
        anchor,
        name,
        &gettext("_Rename"),
        glib::clone!(
            #[weak]
            store,
            move |name| {
                let update = CategoryUpdate {
                    name: Some(name),
                    ..Default::default()
                };
                glib::spawn_future_local(async move {
                    store.update_category(id, update).await;
                });
            }
        ),
    );
}

fn merge_category(parent: &adw::Dialog, store: &Store, id: i64, name: &str) {
    let others: Vec<_> = store
        .ordered_categories()
        .into_iter()
        .filter(|c| c.id != id)
        .collect();
    if others.is_empty() {
        return;
    }
    let labels: Vec<&str> = others.iter().map(|c| c.name.as_str()).collect();
    let target_row = adw::ComboRow::builder()
        .title(gettext("Merge Into"))
        .model(&gtk::StringList::new(&labels))
        .build();
    let group = adw::PreferencesGroup::new();
    group.add(&target_row);

    let dialog = adw::AlertDialog::new(
        Some(&gettext("Merge “%s”?").replace("%s", name)),
        Some(
            &gettext("Its items move to the chosen category, and “%s” is removed.")
                .replace("%s", name),
        ),
    );
    dialog.set_extra_child(Some(&group));
    dialog.add_responses(&[
        ("cancel", &gettext("_Cancel")),
        ("merge", &gettext("_Merge")),
    ]);
    dialog.set_response_appearance("merge", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(
        Some("merge"),
        glib::clone!(
            #[weak]
            store,
            move |_, _| {
                let Some(target) = others.get(target_row.selected() as usize) else {
                    return;
                };
                // The server folds the given category into the one being updated.
                let update = CategoryUpdate {
                    merge_category_id: Some(id),
                    ..Default::default()
                };
                let target = target.id;
                glib::spawn_future_local(async move {
                    store.update_category(target, update).await;
                });
            }
        ),
    );
    dialog.present(Some(parent));
}

fn delete_category(parent: &adw::Dialog, store: &Store, id: i64, name: &str) {
    let count = store
        .items()
        .iter()
        .filter(|i| i.category.as_ref().is_some_and(|c| c.id == id))
        .count();
    let body = if count == 0 {
        gettext("No items use it.")
    } else {
        gettext("Its %d items will become uncategorised.").replace("%d", &count.to_string())
    };
    widgets::confirm(
        parent,
        &gettext("Delete “%s”?").replace("%s", name),
        &body,
        &gettext("_Delete"),
        glib::clone!(
            #[weak]
            store,
            move || {
                glib::spawn_future_local(async move {
                    store.delete_category(id).await;
                });
            }
        ),
    );
}
