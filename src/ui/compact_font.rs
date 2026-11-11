//! Compact density uses smaller text. A percentage gives a fractional pixel size, which makes
//! text shimmer and clip, so the size is worked out from the system font and rounded to whole
//! pixels. It is recomputed when the font or text scaling changes.

use gtk::{gdk, pango};

const SCALE: f64 = 0.9;

pub fn install() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let settings = gtk::Settings::for_display(&display);
    let provider = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let update = move |settings: &gtk::Settings| provider.load_from_string(&css(base_px(settings)));
    update(&settings);
    let on_font = update.clone();
    settings.connect_gtk_font_name_notify(move |s| on_font(s));
    settings.connect_gtk_xft_dpi_notify(move |s| update(s));
}

fn base_px(settings: &gtk::Settings) -> f64 {
    let name = settings
        .gtk_font_name()
        .map_or_else(|| "Sans 11".to_owned(), |n| n.to_string());
    let font = pango::FontDescription::from_string(&name);
    let size = f64::from(font.size()) / f64::from(pango::SCALE);
    if font.is_size_absolute() {
        return size;
    }
    // The DPI setting carries the text scaling factor, in 1/1024ths.
    let dpi = match settings.gtk_xft_dpi() {
        dpi if dpi > 0 => f64::from(dpi) / 1024.0,
        _ => 96.0,
    };
    size * dpi / 72.0
}

fn css(base_px: f64) -> String {
    let small = (base_px * SCALE).round().max(1.0);
    format!(
        ".owlet-list.compact .owlet-item-name, .owlet-list.compact .owlet-category \
         {{ font-size: {small}px; }}"
    )
}
