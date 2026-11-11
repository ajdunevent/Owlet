use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::store::Store;

/// A combo row whose choices map to a GSettings enum's nicks, in order.
fn enum_row(
    settings: &gio::Settings,
    key: &str,
    title: &str,
    choices: &[(&str, String)],
) -> adw::ComboRow {
    let labels: Vec<&str> = choices.iter().map(|(_, label)| label.as_str()).collect();
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&labels))
        .build();
    let current = settings.string(key);
    if let Some(index) = choices
        .iter()
        .position(|(nick, _)| *nick == current.as_str())
    {
        row.set_selected(index as u32);
    }
    let nicks: Vec<String> = choices.iter().map(|(nick, _)| (*nick).to_owned()).collect();
    let key = key.to_owned();
    row.connect_selected_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            if let Some(nick) = nicks.get(row.selected() as usize) {
                let _ = settings.set_string(&key, nick);
            }
        }
    ));
    row
}

pub fn show_preferences(parent: &impl IsA<gtk::Widget>, store: &Store) {
    let settings = store.settings();
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Shopping List"))
        .description(gettext("These settings apply to this device only"))
        .build();
    group.add(&enum_row(
        &settings,
        "tap-mode",
        &gettext("Tapping an Item"),
        &[
            ("instant", gettext("Checks It Off")),
            ("select", gettext("Selects It")),
        ],
    ));
    group.add(&enum_row(
        &settings,
        "sort-order",
        &gettext("Sort Items By"),
        &[
            ("category", gettext("Category")),
            ("alphabetical", gettext("Alphabetical")),
            ("algorithmic", gettext("Algorithmic")),
        ],
    ));
    group.add(&enum_row(
        &settings,
        "density",
        &gettext("List Density"),
        &[
            ("regular", gettext("Regular")),
            ("compact", gettext("Compact")),
        ],
    ));

    let page = adw::PreferencesPage::new();
    page.add(&group);
    let dialog = adw::PreferencesDialog::new();
    dialog.add(&page);
    dialog.present(Some(parent));
}

pub fn show_profile(parent: &impl IsA<gtk::Widget>, store: &Store) {
    let user = store.user();
    let group = adw::PreferencesGroup::new();
    let rows = [
        (gettext("Name"), user.as_ref().map(|u| u.name.clone())),
        (
            gettext("Username"),
            user.as_ref().map(|u| u.username.clone()),
        ),
        (gettext("Server"), store.server_url()),
    ];
    for (title, value) in rows {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(glib::markup_escape_text(&value.unwrap_or_default()))
            .subtitle_selectable(true)
            .build();
        row.add_css_class("property");
        group.add(&row);
    }

    let log_out = gtk::Button::builder()
        .label(gettext("_Log Out"))
        .use_underline(true)
        .halign(gtk::Align::Center)
        .build();
    log_out.add_css_class("pill");
    log_out.add_css_class("destructive-action");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(12);
    content.set_margin_bottom(24);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&group);
    content.append(&log_out);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(
        &adw::Clamp::builder()
            .maximum_size(420)
            .child(&content)
            .build(),
    ));

    let dialog = adw::Dialog::builder()
        .title(gettext("Profile"))
        .content_width(420)
        .child(&view)
        .build();
    log_out.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        #[weak]
        store,
        move |button| {
            button.set_sensitive(false);
            glib::spawn_future_local(async move {
                store.sign_out().await;
                dialog.close();
            });
        }
    ));
    dialog.present(Some(parent));
}

pub fn show_shortcuts(parent: &impl IsA<gtk::Widget>) {
    let general = adw::ShortcutsSection::new(Some(&gettext("General")));
    for (title, accel) in [
        (gettext("Refresh"), "<Control>r"),
        (gettext("Search Items"), "<Control>f"),
        (gettext("Show Shortcuts"), "<Control>question"),
        (gettext("Close Window"), "<Control>w"),
        (gettext("Quit"), "<Control>q"),
    ] {
        general.add(adw::ShortcutsItem::new(&title, accel));
    }
    let dialog = adw::ShortcutsDialog::new();
    dialog.add(general);
    dialog.present(Some(parent));
}
