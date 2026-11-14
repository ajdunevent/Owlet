# Developing Owlet

Owlet is written in Rust with gtk4-rs and libadwaita, and builds with Meson as a Flatpak on the GNOME 51 runtime.

## Building

```sh
build-aux/dev.sh
```

`dev.sh` asks for the architecture and whether to write a `.flatpak` bundle to `dist/`, install for the current user, or both. It installs the GNOME 51 SDK and the Rust extension from Flathub if they're missing. Builds are incremental.

You need `flatpak` with the Flathub remote, and either the `org.flatpak.Builder` Flatpak or `flatpak-builder`. Cargo fetches crates during the build, so it needs network access.

## Tests

```sh
cargo test
```

Unit and fixture tests run anywhere the GNOME SDK libraries are available. One test checks that every icon Owlet names exists in its bundled resources or the GNOME runtime; it runs only inside the SDK.

The live API test runs only when a server is given. It writes to that server, so use a throwaway one:

```sh
OWLET_TEST_SERVER=http://127.0.0.1:5000 OWLET_TEST_USER=tester OWLET_TEST_PASSWORD=tester-pass cargo test
```

The fixtures in `src/api/testdata/` come from a local KitchenOwl 0.7.10 test server.

## Item icons

Item icons are KitchenOwl's Icons8 set, fetched from Icons8 as 100 px PNGs, which Icons8 allows free with attribution:

```sh
build-aux/fetch-item-icons.py
```

It reads the icon names from `data/item-icons.txt` (taken from KitchenOwl's `item_icons.dart`), saves each as `data/resources/item-icons/<name>.png`, rewrites `item-icons.gresource.xml` to match, keeps files already present unless given `--force`, and lists any it could not fetch.

Most names match an iOS-style icon on Icons8 directly. Where KitchenOwl's drawing has another name on Icons8, the second field in `data/item-icons.txt` gives it, or `id:<id>` for a drawing from another style. KitchenOwl's pasta, penne, tofu and trash_bag drawings are not on Icons8, and its icon folder may not be reused, so Owlet draws its own in `data/item-icon-drawings/`. Names marked `drawn` are rendered from there with `rsvg-convert`; draw them on a 50 px grid with 2 px round strokes to match the iOS style.

The icons ship as a separate `item-icons.gresource`. They are black on transparency and drawn in the text colour, so they follow dark mode.

## Layout

| Path | Contents |
| --- | --- |
| `src/api/` | KitchenOwl REST and Socket.IO client, with no GTK, so its tests run without a display |
| `src/store.rs` | App state, the offline queue and live updates |
| `src/cache.rs` | SQLite cache of the last data shown and queued changes |
| `src/ui/` | Windows, pages and dialogs |
| `data/resources/ui/` | Blueprint templates |

## Releases

Adding a `<release>` to the top of the metainfo, with the same version in `Cargo.toml`, `Cargo.lock` and `meson.build`, and pushing to `main` tags the release on Forgejo. The GitHub mirror then builds and publishes the x86_64 and aarch64 bundles.
