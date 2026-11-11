use std::cell::Cell;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ChipFlow {
        pub spacing: Cell<i32>,
        pub line_spacing: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ChipFlow {
        const NAME: &'static str = "OwletChipFlow";
        type Type = super::ChipFlow;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ChipFlow {
        fn dispose(&self) {
            self.obj().remove_all();
        }
    }

    impl WidgetImpl for ChipFlow {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            let chips = obj.chips();
            if orientation == gtk::Orientation::Horizontal {
                let min = chips.iter().map(|c| c.measure(orientation, -1).0).max();
                let natural: i32 = chips
                    .iter()
                    .map(|c| c.measure(orientation, -1).1)
                    .sum::<i32>()
                    + self.spacing.get() * (chips.len() as i32 - 1).max(0);
                return (min.unwrap_or(0), natural, -1, -1);
            }
            let width = if for_size < 0 { i32::MAX } else { for_size };
            let height = obj.rows(width).iter().map(|row| row.height).sum::<i32>()
                + self.line_spacing.get() * (obj.rows(width).len() as i32 - 1).max(0);
            (height, height, -1, -1)
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            let mut y = 0;
            for row in self.obj().rows(width) {
                let mut x = 0;
                for (chip, chip_width) in &row.chips {
                    chip.size_allocate(&gtk::Allocation::new(x, y, *chip_width, row.height), -1);
                    x += chip_width + self.spacing.get();
                }
                y += row.height + self.line_spacing.get();
            }
        }
    }
}

glib::wrapper! {
    /// Lays chips out in rows, putting each into the first row with room for it, so a short chip
    /// fills a gap left above rather than starting a new row. Rows keep the chips' order.
    pub struct ChipFlow(ObjectSubclass<imp::ChipFlow>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

struct Row {
    chips: Vec<(gtk::Widget, i32)>,
    used: i32,
    height: i32,
}

impl Default for ChipFlow {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl ChipFlow {
    pub fn set_spacing(&self, spacing: i32) {
        self.imp().spacing.set(spacing);
        self.queue_resize();
    }

    pub fn set_line_spacing(&self, spacing: i32) {
        self.imp().line_spacing.set(spacing);
        self.queue_resize();
    }

    pub fn append(&self, chip: &impl IsA<gtk::Widget>) {
        chip.set_parent(self);
    }

    pub fn remove_all(&self) {
        while let Some(child) = self.first_child() {
            child.unparent();
        }
    }

    fn chips(&self) -> Vec<gtk::Widget> {
        let mut chips = Vec::new();
        let mut child = self.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if widget.should_layout() {
                chips.push(widget);
            }
        }
        chips
    }

    fn rows(&self, width: i32) -> Vec<Row> {
        let spacing = self.imp().spacing.get();
        let mut rows: Vec<Row> = Vec::new();
        for chip in self.chips() {
            let chip_width = chip.measure(gtk::Orientation::Horizontal, -1).1.min(width);
            let height = chip.measure(gtk::Orientation::Vertical, chip_width).1;
            let fits = rows
                .iter_mut()
                .find(|row| row.used + spacing + chip_width <= width);
            match fits {
                Some(row) => {
                    row.used += spacing + chip_width;
                    row.height = row.height.max(height);
                    row.chips.push((chip, chip_width));
                }
                None => rows.push(Row {
                    chips: vec![(chip, chip_width)],
                    used: chip_width,
                    height,
                }),
            }
        }
        rows
    }
}
