// Some endpoints are not wired to the UI yet.
#[allow(dead_code, unused_imports)]
mod api;
mod application;
mod cache;
#[cfg(test)]
mod icon_names;
#[rustfmt::skip]
#[allow(dead_code)]
mod config;
mod secrets;
mod store;
mod ui;

use gettextrs::{LocaleCategory, gettext};
use gtk::prelude::*;
use gtk::{gio, glib};

use self::application::Application;
use self::config::{GETTEXT_PACKAGE, LOCALEDIR, RESOURCES_FILE};

fn main() -> glib::ExitCode {
    init_logging();

    gettextrs::setlocale(LocaleCategory::LcAll, "");
    gettextrs::bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR).expect("Unable to bind the text domain");
    gettextrs::bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    gettextrs::textdomain(GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    let resources = gio::Resource::load(RESOURCES_FILE).expect("Could not load resources");
    gio::resources_register(&resources);

    glib::set_application_name(&gettext("Owlet"));

    Application::new().run()
}

/// Forwards the `log` crate to GLib logging under the `owlet` domain. Info and debug output
/// appear with `G_MESSAGES_DEBUG=owlet`.
fn init_logging() {
    struct GlibLogger(log::LevelFilter);

    impl log::Log for GlibLogger {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= self.0
        }

        fn log(&self, record: &log::Record) {
            if !self.enabled(record.metadata()) {
                return;
            }
            let level = match record.level() {
                log::Level::Error => glib::LogLevel::Critical,
                log::Level::Warn => glib::LogLevel::Warning,
                log::Level::Info => glib::LogLevel::Info,
                log::Level::Debug | log::Level::Trace => glib::LogLevel::Debug,
            };
            glib::g_log!("owlet", level, "{}", record.args());
        }

        fn flush(&self) {}
    }

    let filter = if config::PROFILE == "Devel" {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    if log::set_boxed_logger(Box::new(GlibLogger(filter))).is_ok() {
        log::set_max_level(filter);
    }
}
