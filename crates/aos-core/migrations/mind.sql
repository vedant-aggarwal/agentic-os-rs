-- Mind OS: the library of saved things (design/specs/mind-os.md section 7).
-- Idempotent (IF NOT EXISTS) and run on every open, like mission.sql, so parallel branches
-- never fight over user_version.

-- One saved thing. `raw` is exactly what the owner (or the share sheet) gave and is never
-- rewritten; Claude's reading lives in title/description/summary/tags next to it.
CREATE TABLE IF NOT EXISTS mind_item (
    id             TEXT PRIMARY KEY,
    kind           TEXT NOT NULL CHECK (kind IN ('link', 'image', 'note', 'quote', 'video', 'file',
                                                  'person', 'book', 'decision', 'learning', 'daily')),
    raw            TEXT NOT NULL DEFAULT '',     -- his words or the shared text, verbatim
    title          TEXT,                          -- his title, else fetched, else Claude's
    description    TEXT,                          -- Claude: what it is and what is in it
    summary        TEXT,                          -- Claude: short recap (articles, long notes)
    body_text      TEXT,                          -- readable text fetched from a link
    source_url     TEXT,
    source_domain  TEXT,
    site_name      TEXT,
    image_url      TEXT,                          -- og:image of a link, before it is downloaded
    asset_id       TEXT,                          -- original file (asset table, media folder)
    thumb_asset_id TEXT,                          -- <= 512 px JPEG, what cards and Claude see
    width          INTEGER,
    height         INTEGER,
    color          TEXT,                          -- dominant colour, #rrggbb (hover glow)
    happened_on    TEXT,                          -- the day it is about (daily, decision), optional
    origin         TEXT NOT NULL DEFAULT 'app',   -- capture | voice | paste | share | api | nora | board
    status         TEXT NOT NULL DEFAULT 'saved', -- saved | fetching | reading | ready | failed
    inbox          INTEGER NOT NULL DEFAULT 1,    -- 1 until he opens or keeps it
    pinned         INTEGER NOT NULL DEFAULT 0,
    sensitive      INTEGER NOT NULL DEFAULT 0,    -- never sent to Claude, hidden in Safe mode
    archived       INTEGER NOT NULL DEFAULT 0,
    superseded_by  TEXT,                          -- a newer item replaced this one (kept, closed)
    model          TEXT,
    prompt_version TEXT,
    error          TEXT,
    saved_by       TEXT NOT NULL DEFAULT '',
    saved_at       TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    deleted_at     TEXT
);
CREATE INDEX IF NOT EXISTS mind_item_saved ON mind_item(saved_at);

-- Tags: Claude's and his. His win; a tag he removes is kept as `removed` so Claude never re-adds it.
CREATE TABLE IF NOT EXISTS mind_tag (
    item_id TEXT NOT NULL REFERENCES mind_item(id) ON DELETE CASCADE,
    tag     TEXT NOT NULL,
    by      TEXT NOT NULL DEFAULT 'owner',      -- owner | claude
    removed INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (item_id, tag)
);
CREATE INDEX IF NOT EXISTS mind_tag_tag ON mind_tag(tag);

-- Links out of an item: [[...]] references to other items (to_id filled once resolved), and
-- links to boards, goals, projects, tasks. Backlinks are the same rows read the other way.
CREATE TABLE IF NOT EXISTS mind_edge (
    id         TEXT PRIMARY KEY,
    from_id    TEXT NOT NULL REFERENCES mind_item(id) ON DELETE CASCADE,
    to_kind    TEXT NOT NULL,                   -- mind_item | board | mc_goal | dream | project | task | journal
    to_id      TEXT,
    to_title   TEXT,                            -- the [[text]] as written, for unresolved links
    rel        TEXT NOT NULL DEFAULT 'mentions', -- mentions | about | part_of | learned_from | supersedes | contradicts | supports
    by         TEXT NOT NULL DEFAULT 'owner',
    status     TEXT NOT NULL DEFAULT 'active',   -- active | suggested | rejected
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS mind_edge_from ON mind_edge(from_id);
CREATE INDEX IF NOT EXISTS mind_edge_to ON mind_edge(to_kind, to_id);

-- Nora's additions to an item: appended, dated, never mixed into his words.
CREATE TABLE IF NOT EXISTS mind_note (
    id      TEXT PRIMARY KEY,
    item_id TEXT NOT NULL REFERENCES mind_item(id) ON DELETE CASCADE,
    text    TEXT NOT NULL,
    by      TEXT NOT NULL,
    at      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS mind_note_item ON mind_note(item_id);

-- The enrichment queue: fetch (links) -> thumb (images) -> tag (Claude, low priority).
CREATE TABLE IF NOT EXISTS mind_job (
    item_id   TEXT NOT NULL REFERENCES mind_item(id) ON DELETE CASCADE,
    stage     TEXT NOT NULL CHECK (stage IN ('fetch', 'thumb', 'tag')),
    state     TEXT NOT NULL DEFAULT 'queued',   -- queued | done | failed | skipped
    tries     INTEGER NOT NULL DEFAULT 0,
    error     TEXT,
    run_after TEXT NOT NULL,
    PRIMARY KEY (item_id, stage)
);

-- Full-text search over everything readable. Kept in step by the store (not triggers), because
-- tags and notes live in other tables.
CREATE VIRTUAL TABLE IF NOT EXISTS mind_fts USING fts5(
    item_id UNINDEXED, title, raw, description, summary, tags, body, source,
    tokenize = 'unicode61 remove_diacritics 2'
);
