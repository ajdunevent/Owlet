use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;

/// Names an icon-only control for screen readers; GTK reports a tooltip only as a
/// description.
pub fn name(widget: &impl IsA<gtk::Accessible>, text: &str) {
    widget.update_property(&[gtk::accessible::Property::Label(text)]);
}

/// A popover with a text field for naming things, anchored to `anchor`. `on_submit` gets the
/// trimmed name; empty or unchanged names are ignored.
pub fn name_popover(
    anchor: &impl IsA<gtk::Widget>,
    initial: &str,
    action_label: &str,
    on_submit: impl Fn(String) + 'static,
) {
    let entry = gtk::Entry::builder()
        .text(initial)
        .activates_default(true)
        .width_chars(20)
        .build();
    let button = gtk::Button::builder()
        .label(action_label)
        .use_underline(true)
        .sensitive(false)
        .build();
    button.add_css_class("suggested-action");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&entry);
    content.append(&button);

    let popover = gtk::Popover::builder().child(&content).build();
    popover.set_parent(anchor);
    popover.set_default_widget(Some(&button));

    let initial = initial.to_owned();
    let changed_initial = initial.clone();
    entry.connect_changed(glib::clone!(
        #[weak]
        button,
        move |entry| {
            let text = entry.text();
            let text = text.trim();
            button.set_sensitive(!text.is_empty() && text != changed_initial);
        }
    ));
    let submit = std::rc::Rc::new(on_submit);
    button.connect_clicked(glib::clone!(
        #[weak]
        popover,
        #[weak]
        entry,
        move |_| {
            let text = entry.text().trim().to_owned();
            if !text.is_empty() && text != initial {
                submit(text);
            }
            popover.popdown();
        }
    ));
    popover.connect_closed(|popover| {
        let popover = popover.clone();
        glib::idle_add_local_once(move || popover.unparent());
    });
    popover.popup();
    entry.grab_focus();
    entry.select_region(0, -1);
}

/// Asks before something destructive; `on_confirm` runs if the user picks `action_label`.
pub fn confirm(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    action_label: &str,
    on_confirm: impl Fn() + 'static,
) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_responses(&[("cancel", &gettext("_Cancel")), ("confirm", action_label)]);
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(Some("confirm"), move |_, _| on_confirm());
    dialog.present(Some(parent));
}
