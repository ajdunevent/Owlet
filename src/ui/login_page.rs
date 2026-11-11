use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{CompositeTemplate, glib};

use crate::api;
use crate::store::Store;

mod imp {
    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(resource = "/net/dunevent/Owlet/ui/login-page.ui")]
    pub struct LoginPage {
        #[template_child]
        pub form: TemplateChild<gtk::Box>,
        #[template_child]
        pub subtitle_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub server_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub username_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub password_row: TemplateChild<adw::PasswordEntryRow>,
        #[template_child]
        pub error_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub login_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub login_stack: TemplateChild<gtk::Stack>,
        pub store: glib::WeakRef<Store>,
        pub working: std::cell::Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LoginPage {
        const NAME: &'static str = "OwletLoginPage";
        type Type = super::LoginPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[gtk::template_callbacks]
    impl LoginPage {
        #[template_callback]
        fn on_changed(&self) {
            self.obj().update_button();
        }

        #[template_callback]
        fn on_server_activated(&self) {
            self.username_row.grab_focus();
        }

        #[template_callback]
        fn on_username_activated(&self) {
            self.password_row.grab_focus();
        }

        #[template_callback]
        fn on_password_activated(&self) {
            if self.login_button.is_sensitive() {
                self.obj().log_in();
            }
        }

        #[template_callback]
        fn on_login_clicked(&self) {
            self.obj().log_in();
        }
    }

    impl ObjectImpl for LoginPage {}
    impl WidgetImpl for LoginPage {}
    impl BinImpl for LoginPage {}
}

glib::wrapper! {
    pub struct LoginPage(ObjectSubclass<imp::LoginPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LoginPage {
    pub fn set_store(&self, store: &Store) {
        self.imp().store.set(Some(store));
    }

    /// On phones the on-screen keyboard resizes the window as it opens and closes, which
    /// would make a centred form jump; at the top it stays put.
    pub fn set_narrow(&self, narrow: bool) {
        self.imp().form.set_valign(if narrow {
            gtk::Align::Start
        } else {
            gtk::Align::Center
        });
    }

    pub fn reset(&self) {
        let imp = self.imp();
        imp.password_row.set_text("");
        imp.error_label.set_visible(false);
        if let Some(store) = imp.store.upgrade() {
            imp.subtitle_label.set_label(&if store.session_expired() {
                gettext("You were logged out on the server. Log in again to continue.")
            } else {
                gettext("Log in to your KitchenOwl server")
            });
            let settings = store.settings();
            if imp.server_row.text().is_empty() {
                imp.server_row.set_text(&settings.string("server-url"));
            }
            if imp.username_row.text().is_empty() {
                imp.username_row.set_text(&settings.string("username"));
            }
        }
    }

    fn update_button(&self) {
        let imp = self.imp();
        let filled = !imp.server_row.text().trim().is_empty()
            && !imp.username_row.text().trim().is_empty()
            && !imp.password_row.text().is_empty();
        imp.login_button.set_sensitive(filled && !imp.working.get());
    }

    fn set_working(&self, working: bool) {
        let imp = self.imp();
        imp.working.set(working);
        imp.login_stack
            .set_visible_child_name(if working { "spinner" } else { "label" });
        for row in [
            imp.server_row.upcast_ref::<gtk::Widget>(),
            imp.username_row.upcast_ref(),
            imp.password_row.upcast_ref(),
        ] {
            row.set_sensitive(!working);
        }
        self.update_button();
    }

    fn log_in(&self) {
        let Some(store) = self.imp().store.upgrade() else {
            return;
        };
        let imp = self.imp();
        let server = imp.server_row.text().trim().to_owned();
        let username = imp.username_row.text().trim().to_owned();
        let password = imp.password_row.text().to_string();
        imp.error_label.set_visible(false);
        self.set_working(true);
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = page)]
            self,
            async move {
                let result = store.sign_in(&server, &username, &password).await;
                page.set_working(false);
                if let Err(err) = result {
                    let imp = page.imp();
                    imp.error_label.set_label(&error_text(&err));
                    imp.error_label.set_visible(true);
                    if err == api::Error::Unauthorized {
                        imp.password_row.set_text("");
                        imp.password_row.grab_focus();
                    }
                }
            }
        ));
    }
}

fn error_text(err: &api::Error) -> String {
    match err {
        api::Error::InvalidUrl(_) => gettext("That doesn't look like a server address"),
        api::Error::Network(_) => {
            gettext("Can't reach the server. Check the address and your connection.")
        }
        api::Error::NotKitchenOwl => gettext("That address isn't a KitchenOwl server"),
        api::Error::ServerTooOld { .. } => {
            gettext("This KitchenOwl server is too old. Owlet needs KitchenOwl 0.7.10 or newer.")
        }
        api::Error::Unauthorized => gettext("Wrong username or password"),
        other => {
            log::warn!("Login failed: {other}");
            gettext("Couldn't log in. The server reported an error.")
        }
    }
}
