-- Extension tables, applied by db::migrate right after ../schema.sql in the same transaction.
-- Owned by the extensions module. Same conventions as ../schema.sql.
-- These are the v1 definitions. Migration v4 (schema_v4.sql) rebuilt both tables to add
-- 'edge_addons' to the store CHECKs, and holds their current definitions.

-- ── Extensions: desired (Kind::Extensions) vs actual (LOCAL) ─────────────────────────
CREATE TABLE extensions (                -- store-origin extensions only
  id            TEXT PRIMARY KEY,
  store         TEXT NOT NULL CHECK (store IN ('chrome_web_store', 'amo')),
  store_at      BLOB NOT NULL,
  installed     INTEGER NOT NULL,  installed_at  BLOB NOT NULL,
  enabled       INTEGER NOT NULL,  enabled_at    BLOB NOT NULL,
  extra         TEXT NOT NULL DEFAULT '{}',
  seq           INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX extensions_seq ON extensions(seq);

CREATE TABLE extension_installs (        -- LOCAL: what is on this device's disk
  id             TEXT PRIMARY KEY,
  version        TEXT NOT NULL,
  dir            TEXT NOT NULL,          -- '<id>/<version>_<hash32>' under <root>/extensions for managed installs; absolute for unpacked
  source_kind    TEXT NOT NULL CHECK (source_kind IN ('chrome_web_store', 'amo', 'crx_file', 'xpi_file', 'unpacked')),
  source         TEXT NOT NULL,          -- JSON extensions::InstallSource
  verification   TEXT NOT NULL,          -- JSON extensions::Verification: how this device checked the files
  manifest       TEXT NOT NULL,          -- JSON extensions::manifest::Manifest, parsed + localized at install
  -- Who owns `enabled`: the synced `extensions` row for store installs, this column otherwise.
  local_enabled  INTEGER,
  engine_id      TEXT,                   -- WebView2's id for this dir; NULL until the engine loaded this dir
  installed_ms   INTEGER NOT NULL,       -- when this dir was first committed
  CHECK ((local_enabled IS NULL) = (source_kind IN ('chrome_web_store', 'amo')))
) WITHOUT ROWID;
