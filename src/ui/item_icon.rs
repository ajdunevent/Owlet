//! KitchenOwl item icons, drawn from the bundled Icons8 PNGs in the text colour.

use std::cell::RefCell;
use std::collections::HashMap;

use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk, prelude::*};

const RESOURCE_PREFIX: &str = "/net/dunevent/Owlet/item-icons/";

mod imp {
    use std::cell::OnceCell;

    use super::*;

    #[derive(Default)]
    pub struct ItemIcon {
        pub texture: OnceCell<gdk::Texture>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ItemIcon {
        const NAME: &'static str = "OwletItemIcon";
        type Type = super::ItemIcon;
        type Interfaces = (gdk::Paintable, gtk::SymbolicPaintable);
    }

    impl ObjectImpl for ItemIcon {}

    impl PaintableImpl for ItemIcon {
        fn flags(&self) -> gdk::PaintableFlags {
            gdk::PaintableFlags::STATIC_SIZE | gdk::PaintableFlags::STATIC_CONTENTS
        }

        fn intrinsic_width(&self) -> i32 {
            self.texture.get().map_or(0, |t| t.width())
        }

        fn intrinsic_height(&self) -> i32 {
            self.texture.get().map_or(0, |t| t.height())
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            self.snapshot_symbolic(snapshot, width, height, &[]);
        }
    }

    impl SymbolicPaintableImpl for ItemIcon {
        fn snapshot_symbolic(
            &self,
            snapshot: &gdk::Snapshot,
            width: f64,
            height: f64,
            colors: &[gdk::RGBA],
        ) {
            let (Some(texture), Some(snapshot)) =
                (self.texture.get(), snapshot.downcast_ref::<gtk::Snapshot>())
            else {
                return;
            };
            let foreground = colors.first().copied().unwrap_or(gdk::RGBA::BLACK);
            // The PNGs are black shapes on transparency, so only their alpha is kept.
            let matrix = graphene::Matrix::from_float([
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                foreground.alpha(),
            ]);
            let offset =
                graphene::Vec4::new(foreground.red(), foreground.green(), foreground.blue(), 0.0);
            snapshot.push_color_matrix(&matrix, &offset);
            snapshot.append_scaled_texture(
                texture,
                gsk::ScalingFilter::Trilinear,
                &graphene::Rect::new(0.0, 0.0, width as f32, height as f32),
            );
            snapshot.pop();
        }
    }
}

glib::wrapper! {
    pub struct ItemIcon(ObjectSubclass<imp::ItemIcon>)
        @implements gdk::Paintable, gtk::SymbolicPaintable;
}

thread_local! {
    static LOADED: RefCell<HashMap<String, Option<ItemIcon>>> = RefCell::new(HashMap::new());
}

/// Every KitchenOwl icon name that has a bundled drawing, sorted.
pub fn names() -> Vec<String> {
    let mut names: Vec<String> =
        gio::resources_enumerate_children(RESOURCE_PREFIX, gio::ResourceLookupFlags::NONE)
            .map(|children| {
                children
                    .iter()
                    .filter_map(|file| file.strip_suffix(".png").map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
    names.sort();
    names
}

/// The drawing for a KitchenOwl icon name, or `None` when Owlet has none for it.
pub fn lookup(name: &str) -> Option<ItemIcon> {
    LOADED.with_borrow_mut(|loaded| {
        loaded
            .entry(name.to_owned())
            .or_insert_with(|| {
                let path = format!("{RESOURCE_PREFIX}{name}.png");
                // Loading a missing resource aborts, so its presence is checked first.
                gio::resources_get_info(&path, gio::ResourceLookupFlags::NONE).ok()?;
                let texture = gdk::Texture::from_resource(&path);
                let icon: ItemIcon = glib::Object::new();
                icon.imp().texture.set(texture).ok();
                Some(icon)
            })
            .clone()
    })
}

/// Whether Owlet can draw this item's icon.
pub fn has(name: Option<&str>) -> bool {
    name.and_then(lookup).is_some()
}

/// Shows the item's icon in `image`. Without one, the image keeps its space when `reserve` is
/// set, so names line up with rows that have icons, and is hidden otherwise.
pub fn show(image: &gtk::Image, name: Option<&str>, reserve: bool) {
    let icon = name.and_then(lookup);
    image.set_paintable(icon.as_ref());
    let size = image.pixel_size();
    image.set_size_request(size, size);
    image.set_visible(icon.is_some() || reserve);
}
