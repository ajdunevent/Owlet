use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::api::{Item, ItemUpdate};
use crate::store::Store;
use crate::ui::widgets;

/// The item as it sits on the list the editor was opened from.
pub struct OnList {
    pub list: i64,
    pub description: String,
}

pub fn show_item_editor(
    parent: &impl IsA<gtk::Widget>,
    store: &Store,
    item: &Item,
    on_list: Option<OnList>,
) {
    let categories = store.ordered_categories();
    let page = adw::PreferencesPage::new();

    let description_row = on_list.as_ref().map(|entry| {
        let row = adw::EntryRow::builder()
            .title(gettext("Description"))
            .text(&entry.description)
            .build();
        let group = adw::PreferencesGroup::builder()
            .title(gettext("On This List"))
            .build();
        group.add(&row);
        page.add(&group);
        row
    });

    let name_row = adw::EntryRow::builder()
        .title(gettext("Name"))
        .text(&item.name)
        .build();
    let mut category_names = vec![gettext("None")];
    category_names.extend(categories.iter().map(|c| c.name.clone()));
    let category_labels: Vec<&str> = category_names.iter().map(String::as_str).collect();
    let category_row = adw::ComboRow::builder()
        .title(gettext("Category"))
        .model(&gtk::StringList::new(&category_labels))
        .build();
    let current_category = item
        .category
        .as_ref()
        .and_then(|c| categories.iter().position(|known| known.id == c.id))
        .map_or(0, |i| i as u32 + 1);
    category_row.set_selected(current_category);
    // Reserved for item icons; stays hidden until icon artwork is bundled.
    let icon_row = adw::ActionRow::builder()
        .title(gettext("Icon"))
        .visible(false)
        .build();
    let item_group = adw::PreferencesGroup::builder()
        .title(gettext("Item"))
        .build();
    item_group.add(&name_row);
    item_group.add(&category_row);
    item_group.add(&icon_row);
    page.add(&item_group);

    let actions = adw::PreferencesGroup::new();
    let remove_row = on_list.as_ref().map(|_| {
        let row = adw::ButtonRow::builder()
            .title(gettext("_Remove from List"))
            .use_underline(true)
            .build();
        actions.add(&row);
        row
    });
    let merge_row = adw::ButtonRow::builder()
        .title(gettext("_Merge Into…"))
        .use_underline(true)
        .build();
    let delete_row = adw::ButtonRow::builder()
        .title(gettext("_Delete Item"))
        .use_underline(true)
        .build();
    delete_row.add_css_class("destructive-action");
    actions.add(&merge_row);
    actions.add(&delete_row);
    page.add(&actions);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&page));
    let dialog = adw::Dialog::builder()
        .title(gettext("Edit Item"))
        .content_width(420)
        .content_height(560)
        .child(&view)
        .build();

    // Set when the item is gone, so closing does not try to save it.
    let finished = Rc::new(Cell::new(false));
    let item_id = item.id;

    if let (Some(row), Some(entry)) = (&remove_row, &on_list) {
        let list = entry.list;
        row.connect_activated(glib::clone!(
            #[weak]
            dialog,
            #[weak]
            store,
            #[strong]
            finished,
            move |_| {
                finished.set(true);
                dialog.close();
                glib::spawn_future_local(async move {
                    store.check_off(list, &[item_id]).await;
                });
            }
        ));
    }

    let item_name = item.name.clone();
    merge_row.connect_activated(glib::clone!(
        #[weak]
        dialog,
        #[weak]
        store,
        #[strong]
        finished,
        move |_| show_merge_picker(&dialog, &store, item_id, &item_name, finished.clone())
    ));

    let item_name = item.name.clone();
    delete_row.connect_activated(glib::clone!(
        #[weak]
        dialog,
        #[weak]
        store,
        #[strong]
        finished,
        move |_| {
            let finished = finished.clone();
            widgets::confirm(
                &dialog,
                &gettext("Delete “%s”?").replace("%s", &item_name),
                &gettext("It will be removed from the catalog and from every list."),
                &gettext("_Delete"),
                glib::clone!(
                    #[weak]
                    dialog,
                    #[weak]
                    store,
                    move || {
                        finished.set(true);
                        dialog.close();
                        glib::spawn_future_local(async move {
                            store.delete_item(item_id).await;
                        });
                    }
                ),
            );
        }
    ));

    let original_name = item.name.clone();
    let original_description = on_list.as_ref().map(|e| e.description.clone());
    let list = on_list.as_ref().map(|e| e.list);
    dialog.connect_closed(glib::clone!(
        #[weak]
        store,
        move |_| {
            if finished.get() {
                return;
            }
            let name = name_row.text().trim().to_owned();
            let category = match category_row.selected() {
                0 => None,
                i => categories.get(i as usize - 1).map(|c| c.id),
            };
            let mut update = ItemUpdate::default();
            if !name.is_empty() && name != original_name {
                update.name = Some(name);
            }
            if category_row.selected() != current_category {
                update.category = Some(category);
            }
            let description = description_row.as_ref().map(|r| r.text().trim().to_owned());
            let description_changed = description != original_description;
            glib::spawn_future_local(async move {
                if let (true, Some(list), Some(description)) =
                    (description_changed, list, description)
                {
                    store.set_description(list, item_id, &description).await;
                }
                if update != ItemUpdate::default() {
                    store.update_item(item_id, update).await;
                }
            });
        }
    ));

    dialog.present(Some(parent));
}

/// Picks another item to fold this one into; the picked item keeps its name.
fn show_merge_picker(
    editor: &adw::Dialog,
    store: &Store,
    item_id: i64,
    item_name: &str,
    finished: Rc<Cell<bool>>,
) {
    let search = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search items"))
        .build();
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    let mut items: Vec<Item> = store
        .items()
        .into_iter()
        .filter(|i| i.id != item_id)
        .collect();
    items.sort_by_key(|i| i.name.to_lowercase());

    let picker = adw::Dialog::builder()
        .title(gettext("Merge Into"))
        .content_width(420)
        .content_height(560)
        .build();

    for target in items {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&target.name))
            .activatable(true)
            .build();
        row.set_widget_name(&target.name.to_lowercase());
        let source_name = item_name.to_owned();
        row.connect_activated(glib::clone!(
            #[weak]
            picker,
            #[weak]
            editor,
            #[weak]
            store,
            #[strong]
            finished,
            move |_| {
                let target_id = target.id;
                let finished = finished.clone();
                widgets::confirm(
                    &picker,
                    &gettext("Merge “%1” into “%2”?")
                        .replace("%1", &source_name)
                        .replace("%2", &target.name),
                    &gettext("Lists and recipes using it will use “%s” instead.")
                        .replace("%s", &target.name),
                    &gettext("_Merge"),
                    glib::clone!(
                        #[weak]
                        picker,
                        #[weak]
                        editor,
                        #[weak]
                        store,
                        move || {
                            finished.set(true);
                            picker.close();
                            editor.close();
                            let update = ItemUpdate {
                                merge_item_id: Some(item_id),
                                ..Default::default()
                            };
                            glib::spawn_future_local(async move {
                                store.update_item(target_id, update).await;
                            });
                        }
                    ),
                );
            }
        ));
        list.append(&row);
    }
    list.set_filter_func(glib::clone!(
        #[weak]
        search,
        #[upgrade_or]
        true,
        move |row| {
            let query = search.text().to_lowercase();
            query.is_empty() || row.widget_name().contains(query.as_str())
        }
    ));
    search.connect_search_changed(glib::clone!(
        #[weak]
        list,
        move |_| list.invalidate_filter()
    ));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(6);
    content.set_margin_bottom(24);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&search);
    content.append(&list);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&content)
            .build(),
    ));
    picker.set_child(Some(&view));
    picker.present(Some(editor));
}
