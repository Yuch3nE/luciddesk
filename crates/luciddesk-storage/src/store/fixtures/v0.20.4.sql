-- Exact schema from v0.20.4 (crates/luciddesk-storage/src/store/schema.rs).

CREATE TABLE metadata (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
CREATE TABLE panels (
    id INTEGER PRIMARY KEY CHECK(id>0), title TEXT NOT NULL,
    x REAL NOT NULL, y REAL NOT NULL, width REAL NOT NULL CHECK(width>0), height REAL NOT NULL CHECK(height>0),
    collapsed INTEGER NOT NULL CHECK(collapsed IN (0,1)), locked INTEGER NOT NULL CHECK(locked IN (0,1)),
    backdrop_kind TEXT NOT NULL CHECK(backdrop_kind IN ('mica','mica_alt','acrylic','mica_tuned','acrylic_tuned','translucent','solid')),
    opacity REAL CHECK(opacity BETWEEN 0 AND 1), color INTEGER CHECK(color BETWEEN 0 AND 16777215),
    theme TEXT NOT NULL DEFAULT 'system' CHECK(theme IN ('system','light','dark')),
    always_on_top INTEGER NOT NULL DEFAULT 0 CHECK(always_on_top IN (0,1)),
    auto_hide INTEGER NOT NULL DEFAULT 0 CHECK(auto_hide IN (0,1)),
    inherit_theme INTEGER NOT NULL DEFAULT 0 CHECK(inherit_theme IN (0,1)),
    inherit_backdrop INTEGER NOT NULL DEFAULT 0 CHECK(inherit_backdrop IN (0,1)),
    kind TEXT NOT NULL DEFAULT 'desktop' CHECK(kind IN ('desktop','folder','search'))
);
CREATE TABLE panel_folder_settings (
    panel_id INTEGER PRIMARY KEY REFERENCES panels(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    view TEXT NOT NULL DEFAULT 'list' CHECK(view IN ('list','icons')),
    sort_column INTEGER NOT NULL DEFAULT 0 CHECK(sort_column BETWEEN 0 AND 3),
    sort_direction TEXT NOT NULL DEFAULT 'asc' CHECK(sort_direction IN ('asc','desc'))
);
CREATE TABLE desktop_items (
    identity_key TEXT PRIMARY KEY NOT NULL,
    identity_kind TEXT NOT NULL CHECK(identity_kind IN ('filesystem','namespace')),
    identity_value TEXT NOT NULL, volume_id TEXT, file_id TEXT, display_name TEXT NOT NULL,
    placement_kind TEXT NOT NULL CHECK(placement_kind IN ('free','pane')),
    monitor_id TEXT, x REAL, y REAL,
    pane_id INTEGER REFERENCES panels(id) DEFERRABLE INITIALLY DEFERRED,
    grid_column INTEGER, grid_row INTEGER,
    CHECK((placement_kind='free' AND monitor_id IS NOT NULL AND x IS NOT NULL AND y IS NOT NULL AND pane_id IS NULL AND grid_column IS NULL AND grid_row IS NULL)
       OR (placement_kind='pane' AND pane_id IS NOT NULL AND grid_column>=0 AND grid_column IS NOT NULL AND grid_row>=0 AND grid_row IS NOT NULL AND monitor_id IS NULL AND ((x IS NULL AND y IS NULL) OR (x IS NOT NULL AND y IS NOT NULL AND x>=0 AND y>=0))))
);
CREATE INDEX desktop_items_pane ON desktop_items(pane_id);
CREATE TABLE monitor_layouts (
    topology TEXT NOT NULL, panel_id INTEGER NOT NULL REFERENCES panels(id) ON DELETE CASCADE,
    x REAL NOT NULL, y REAL NOT NULL, width REAL NOT NULL CHECK(width>0), height REAL NOT NULL CHECK(height>0),
    PRIMARY KEY(topology,panel_id)
);
CREATE INDEX monitor_layouts_panel ON monitor_layouts(panel_id);

CREATE TABLE panel_folder_view (
 panel_id INTEGER PRIMARY KEY REFERENCES panel_folder_settings(panel_id) ON DELETE CASCADE,
 name_width REAL, modified_width REAL, type_width REAL, size_width REAL,
 visible_columns INTEGER CHECK(visible_columns BETWEEN 1 AND 15 AND (visible_columns & 1)=1),
 CHECK((name_width IS NULL AND modified_width IS NULL AND type_width IS NULL AND size_width IS NULL)
 OR (name_width IS NOT NULL AND modified_width IS NOT NULL AND type_width IS NOT NULL AND size_width IS NOT NULL
 AND name_width>0 AND name_width<1 AND modified_width>0 AND modified_width<1
 AND type_width>0 AND type_width<1 AND size_width>0 AND size_width<1
 AND abs(name_width+modified_width+type_width+size_width-1)<0.001))
);