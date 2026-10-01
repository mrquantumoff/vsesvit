-- Migration v4, applied by db::migrate in one transaction: 'edge_addons' joins the store lists.
-- SQLite cannot alter a CHECK, so both tables are rebuilt the way
-- https://sqlite.org/lang_altertable.html#otheralter describes: create the new table, copy,
-- drop the old one, rename the new one into place, recreate the indexes. Nothing references
-- these tables (no foreign keys, triggers or views), so foreign_keys can stay on.
-- These are the tables' current definitions; ./schema.sql holds the v1 ones.

CREATE TABLE extensions_v4 (             -- store-origin extensions only
  id            TEXT PRIMARY KEY,
  store         TEXT NOT NULL CHECK (store IN ('chrome_web_store', 'edge_addons', 'amo')),
  store_at      BLOB NOT NULL,
  installed     INTEGER NOT NULL,  installed_at  BLOB NOT NULL,
  enabled       INTEGER NOT NULL,  enabled_at    BLOB NOT NULL,
  extra         TEXT NOT NULL DEFAULT '{}',
  seq           INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO extensions_v4 (id, store, store_at, installed, installed_at, enabled, enabled_at, extra, seq)
  SELECT id, store, store_at, installed, installed_at, enabled, enabled_at, extra, seq FROM extensions;
DROP TABLE extensions;
ALTER TABLE extensions_v4 RENAME TO extensions;
CREATE INDEX extensions_seq ON extensions(seq);

CREATE TABLE extension_installs_v4 (     -- LOCAL: what is on this device's disk
  id             TEXT PRIMARY KEY,
  version        TEXT NOT NULL,
  dir            TEXT NOT NULL,          -- '<id>/<version>_<hash32>' under <root>/extensions for managed installs; absolute for unpacked
  source_kind    TEXT NOT NULL CHECK (source_kind IN ('chrome_web_store', 'edge_addons', 'amo', 'crx_file', 'xpi_file', 'unpacked')),
  source         TEXT NOT NULL,          -- JSON extensions::InstallSource
  verification   TEXT NOT NULL,          -- JSON extensions::Verification: how this device checked the files
  manifest       TEXT NOT NULL,          -- JSON extensions::manifest::Manifest, parsed + localized at install
  -- Who owns `enabled`: the synced `extensions` row for store installs, this column otherwise.
  local_enabled  INTEGER,
  engine_id      TEXT,                   -- WebView2's id for this dir; NULL until the engine loaded this dir
  installed_ms   INTEGER NOT NULL,       -- when this extension was first installed on this device
  CHECK ((local_enabled IS NULL) = (source_kind IN ('chrome_web_store', 'edge_addons', 'amo')))
) WITHOUT ROWID;
INSERT INTO extension_installs_v4 (id, version, dir, source_kind, source, verification, manifest, local_enabled, engine_id, installed_ms)
  SELECT id, version, dir, source_kind, source, verification, manifest, local_enabled, engine_id, installed_ms FROM extension_installs;
DROP TABLE extension_installs;
ALTER TABLE extension_installs_v4 RENAME TO extension_installs;
