blueprints = custom_target(
  'blueprints',
  input: files(
    'ui/login-page.blp',
    'ui/window.blp',
  ),
  # Each .ui is listed so editing a .blp rebuilds the resource bundle. With
  # the directory as the output, incremental builds keep the old templates.
  output: [
    'login-page.ui',
    'window.ui',
  ],
  command: [
    find_program('blueprint-compiler'),
    'batch-compile',
    '@OUTDIR@',
    '@CURRENT_SOURCE_DIR@' / 'ui',
    '@INPUT@',
  ],
)

resources = gnome.compile_resources(
  'resources',
  'resources.gresource.xml',
  gresource_bundle: true,
  source_dir: [meson.current_build_dir(), meson.current_source_dir()],
  install: true,
  install_dir: pkgdatadir,
  dependencies: blueprints,
)

item_icons = gnome.compile_resources(
  'item-icons',
  'item-icons/item-icons.gresource.xml',
  gresource_bundle: true,
  source_dir: meson.current_source_dir() / 'item-icons',
  install: true,
  install_dir: pkgdatadir,
)
