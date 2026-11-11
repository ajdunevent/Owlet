//! Checks that every symbolic icon named in the code or UI files exists in the bundled
//! resources or the GNOME runtime's Adwaita theme. A missing icon renders as a placeholder
//! without any error, and the host's theme may have icons the runtime lacks.

use std::fs;
use std::path::{Path, PathBuf};

const ADWAITA: &str = "/usr/share/icons/Adwaita";

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "blp")
        ) {
            out.push(path);
        }
    }
}

fn icon_names(text: &str) -> impl Iterator<Item = &str> {
    text.split('"').skip(1).step_by(2).filter(|s| {
        s.ends_with("-symbolic")
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}

fn exists(root: &Path, file: &str) -> bool {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == file) {
                return true;
            }
        }
    }
    false
}

#[test]
fn every_icon_resolves() {
    let adwaita = Path::new(ADWAITA);
    if !adwaita.is_dir() {
        eprintln!("Skipped: {ADWAITA} is only present inside the GNOME SDK");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bundled = root.join("data/resources/icons");
    let mut sources = Vec::new();
    files(&root.join("src"), &mut sources);
    files(&root.join("data/resources/ui"), &mut sources);
    let mut missing = Vec::new();
    for path in sources {
        let text = fs::read_to_string(&path).unwrap();
        for name in icon_names(&text) {
            let file = format!("{name}.svg");
            if name != "owlet-symbolic" && !exists(&bundled, &file) && !exists(adwaita, &file) {
                missing.push(format!("{name} ({})", path.display()));
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "icons not found: {missing:#?}");
}
