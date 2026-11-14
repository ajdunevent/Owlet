use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::config::{APP_ID, VERSION};
use crate::store::Store;
use crate::ui::{Window, dialogs};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Application {
        pub store: Store,
        pub window: glib::WeakRef<Window>,
        pub started: std::cell::Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Application {
        const NAME: &'static str = "OwletApplication";
        type Type = super::Application;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for Application {}

    impl ApplicationImpl for Application {
        fn startup(&self) {
            self.parent_startup();
            crate::ui::compact_font::install();
            let obj = self.obj();
            obj.setup_actions();
            obj.set_accels_for_action("app.quit", &["<Control>q"]);
            obj.set_accels_for_action("app.refresh", &["<Control>r", "F5"]);
            obj.set_accels_for_action("app.shortcuts", &["<Control>question"]);
            obj.set_accels_for_action("win.search", &["<Control>f"]);
            obj.set_accels_for_action("window.close", &["<Control>w"]);
        }

        fn activate(&self) {
            self.parent_activate();
            let obj = self.obj();
            let window = self.window.upgrade().unwrap_or_else(|| {
                let window = Window::new(&obj, &self.store);
                self.window.set(Some(&window));
                window
            });
            window.present();
            if !self.started.replace(true) {
                let store = self.store.clone();
                glib::spawn_future_local(async move { store.restore().await });
            }
        }
    }

    impl GtkApplicationImpl for Application {}
    impl AdwApplicationImpl for Application {}
}

glib::wrapper! {
    pub struct Application(ObjectSubclass<imp::Application>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Application {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", APP_ID)
            .property("resource-base-path", "/net/dunevent/Owlet/")
            .build()
    }

    fn store(&self) -> Store {
        self.imp().store.clone()
    }

    fn window(&self) -> Option<Window> {
        self.imp().window.upgrade()
    }

    fn setup_actions(&self) {
        let quit = gio::ActionEntry::builder("quit")
            .activate(|app: &Self, _, _| app.quit())
            .build();
        let about = gio::ActionEntry::builder("about")
            .activate(|app: &Self, _, _| app.show_about())
            .build();
        let refresh = gio::ActionEntry::builder("refresh")
            .activate(|app: &Self, _, _| {
                let store = app.store();
                if store.signed_in() && !store.busy() {
                    glib::spawn_future_local(async move { store.refresh().await });
                }
            })
            .build();
        let open_web = gio::ActionEntry::builder("open-web")
            .activate(|app: &Self, _, _| {
                if let Some(url) = app.store().server_url() {
                    gtk::UriLauncher::new(&url).launch(
                        app.window().as_ref(),
                        gio::Cancellable::NONE,
                        |result| {
                            if let Err(err) = result {
                                log::warn!("Could not open the web app: {err}");
                            }
                        },
                    );
                }
            })
            .build();
        let preferences = gio::ActionEntry::builder("preferences")
            .activate(|app: &Self, _, _| {
                if let Some(window) = app.window() {
                    dialogs::show_preferences(&window, &app.store());
                }
            })
            .build();
        let profile = gio::ActionEntry::builder("profile")
            .activate(|app: &Self, _, _| {
                if let Some(window) = app.window() {
                    dialogs::show_profile(&window, &app.store());
                }
            })
            .build();
        let shortcuts = gio::ActionEntry::builder("shortcuts")
            .activate(|app: &Self, _, _| {
                if let Some(window) = app.window() {
                    dialogs::show_shortcuts(&window);
                }
            })
            .build();
        self.add_action_entries([
            quit,
            about,
            refresh,
            open_web,
            preferences,
            profile,
            shortcuts,
        ]);
    }

    fn show_about(&self) {
        let dialog = adw::AboutDialog::builder()
            .application_name(gettext("Owlet"))
            .application_icon(APP_ID)
            .developer_name("Owlet contributors")
            .copyright("© 2026 Owlet contributors")
            .version(VERSION)
            .website("https://dev.dunevent.net/aj/Owlet")
            .issue_url("https://dev.dunevent.net/aj/Owlet/issues")
            .license_type(gtk::License::Gpl30)
            .comments(gettext("Shopping lists for KitchenOwl"))
            .build();
        dialog.add_acknowledgement_section(
            Some(&gettext("Item Icons")),
            &["Icons8 https://icons8.com"],
        );
        if let Some(window) = self.active_window() {
            dialog.present(Some(&window));
        }
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}
