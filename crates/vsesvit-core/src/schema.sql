-- vsesvit-core schema v1. db::migrate applies it in one transaction, then sets PRAGMA user_version = 1.
--
-- Conventions for every SYNCED table:
--   <field>_at  BLOB(16)  Stamp of the LWW field it follows: big-endian (hlc u64 ++ device u64).
--                         memcmp order == Stamp order.
--   seq         INTEGER   local change sequence (crdt::Seq), the sync cursor. Indexed.
--   extra       TEXT      JSON object of unknown LWW fields from newer builds: {"name":{"v":..,"at":".."}}
-- LOCAL tables have none of these. Sync cannot see them.
-- Timestamps named *_ms are wall-clock unix milliseconds (facts, not edit stamps).

CREATE TABLE meta (
  key   TEXT PRIMARY KEY,               -- 'device_id' | 'clock_last' | 'next_seq' | 'created_ms'
  value INTEGER NOT NULL
) WITHOUT ROWID;

-- ── Bookmarks (Kind::Bookmarks) ──────────────────────────────────────────────────────
-- Whole table is loaded into bookmarks::Model at open. All reads are served from memory,
-- so there are no url/parent indexes. Roots (uuid 1..4) are implicit and never stored.
CREATE TABLE bookmarks (
  id            BLOB PRIMARY KEY CHECK (length(id) = 16),
  kind          INTEGER NOT NULL CHECK (kind IN (0, 1, 2)),   -- folder | url | separator; immutable per id
  parent        BLOB NOT NULL CHECK (length(parent) = 16),    -- raw placement, NOT the effective parent
  position      TEXT NOT NULL,                                -- fractional index, compared bytewise
  placement_at  BLOB NOT NULL,
  added_ms      INTEGER NOT NULL,
  title         TEXT,
  title_at      BLOB,
  url           TEXT,
  url_at        BLOB,
  deleted_at    BLOB,                                         -- non-NULL: tombstone (terminal)
  extra         TEXT NOT NULL DEFAULT '{}',
  seq           INTEGER NOT NULL,
  CHECK ((deleted_at IS NOT NULL) OR (kind = 2) OR (title IS NOT NULL AND title_at IS NOT NULL)),
  CHECK ((deleted_at IS NOT NULL) OR (kind <> 1) OR (url IS NOT NULL AND url_at IS NOT NULL)),
  CHECK ((deleted_at IS NULL) OR (title IS NULL AND url IS NULL))
) WITHOUT ROWID;
CREATE INDEX bookmarks_seq ON bookmarks(seq);

-- ── History (Kind::HistoryPages, Kind::HistoryDeletions) ─────────────────────────────
CREATE TABLE history_pages (
  url            TEXT PRIMARY KEY,
  url_key        TEXT NOT NULL,          -- lowercased url without scheme and leading "www.": omnibox prefix match
  title          TEXT NOT NULL,
  title_at       BLOB NOT NULL,
  extra          TEXT NOT NULL DEFAULT '{}',
  seq            INTEGER NOT NULL,
  -- derived from history_visits; written only by history::refresh_stats
  visit_count    INTEGER NOT NULL,
  typed_count    INTEGER NOT NULL,
  last_visit_ms  INTEGER NOT NULL,
  frecency       INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX history_pages_seq      ON history_pages(seq);
CREATE INDEX history_pages_url_key  ON history_pages(url_key);
CREATE INDEX history_pages_frecency ON history_pages(frecency DESC);

CREATE TABLE history_visits (            -- grow-only set; part of the page record on the wire
  url         TEXT NOT NULL REFERENCES history_pages(url) ON DELETE CASCADE,
  at_ms       INTEGER NOT NULL,
  device      INTEGER NOT NULL,
  transition  INTEGER NOT NULL,
  PRIMARY KEY (url, at_ms, device)
) WITHOUT ROWID;
CREATE INDEX history_visits_time ON history_visits(at_ms);

CREATE TABLE history_deletions (         -- grow-only set of immutable directives
  id       BLOB PRIMARY KEY CHECK (length(id) = 16),
  url      TEXT,                         -- NULL = all urls
  from_ms  INTEGER NOT NULL,
  to_ms    INTEGER NOT NULL,
  seq      INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX history_deletions_seq ON history_deletions(seq);

-- ── Sessions (Kind::Sessions) ────────────────────────────────────────────────────────
CREATE TABLE device_sessions (           -- one row per device; each device writes only its own
  device      INTEGER PRIMARY KEY,
  snapshot    TEXT,                      -- JSON session::SessionSnapshot; NULL = device forgotten
  snapshot_at BLOB NOT NULL,
  seq         INTEGER NOT NULL
);
CREATE INDEX device_sessions_seq ON device_sessions(seq);

CREATE TABLE tab_restore_state (         -- LOCAL: engine back/forward blobs for this device's tabs
  tab_id  BLOB PRIMARY KEY,
  state   BLOB NOT NULL
) WITHOUT ROWID;

-- ── Preferences (Kind::Prefs) ────────────────────────────────────────────────────────
CREATE TABLE prefs (
  key       TEXT PRIMARY KEY,
  value     TEXT,                        -- canonical JSON; NULL = reset to default
  value_at  BLOB NOT NULL,
  synced    INTEGER NOT NULL CHECK (synced IN (0, 1)),   -- from Pref::scope at write time
  seq       INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX prefs_seq ON prefs(seq) WHERE synced = 1;

-- ── Search engines (Kind::SearchEngines) ─────────────────────────────────────────────
-- Built-ins are not rows until edited. Fields at the zero stamp read through to code values.
-- A row is either live (every *_at present) or a tombstone (deleted_at present, nothing else):
-- the two shapes of search::EngineRecord's Record<T>.
CREATE TABLE search_engines (
  id              TEXT PRIMARY KEY,
  name            TEXT,           name_at        BLOB,
  keyword         TEXT,           keyword_at     BLOB,
  search_url      TEXT,           search_url_at  BLOB,
  suggest_url     TEXT,           suggest_url_at BLOB,
  deleted_at      BLOB,
  extra           TEXT NOT NULL DEFAULT '{}',
  seq             INTEGER NOT NULL,
  CHECK ((deleted_at IS NOT NULL) OR (name IS NOT NULL AND name_at IS NOT NULL AND keyword_at IS NOT NULL
                                      AND search_url IS NOT NULL AND search_url_at IS NOT NULL AND suggest_url_at IS NOT NULL)),
  CHECK ((deleted_at IS NULL) OR (name IS NULL AND name_at IS NULL AND keyword IS NULL AND keyword_at IS NULL
                                  AND search_url IS NULL AND search_url_at IS NULL AND suggest_url IS NULL AND suggest_url_at IS NULL))
) WITHOUT ROWID;
CREATE INDEX search_engines_seq ON search_engines(seq);

-- ── Extension storage ────────────────────────────────────────────────────────────────
CREATE TABLE ext_storage_sync (          -- Kind::ExtStorageSync
  ext       TEXT NOT NULL,
  key       TEXT NOT NULL,
  value     TEXT,                        -- canonical JSON; NULL = removed
  value_at  BLOB NOT NULL,
  seq       INTEGER NOT NULL,
  PRIMARY KEY (ext, key)
) WITHOUT ROWID;
CREATE INDEX ext_storage_sync_seq ON ext_storage_sync(seq);

CREATE TABLE ext_storage_local (         -- LOCAL
  ext    TEXT NOT NULL,
  key    TEXT NOT NULL,
  value  TEXT NOT NULL,
  PRIMARY KEY (ext, key)
) WITHOUT ROWID;

-- ── Sync engine's own state (opaque to core) ─────────────────────────────────────────
CREATE TABLE sync_state (
  key    TEXT PRIMARY KEY,
  value  BLOB NOT NULL
) WITHOUT ROWID;
