-- Mission Control (research/mission-control.md, design/specs/boards.md).
-- Applied on every open with IF NOT EXISTS, outside the numbered migrations, so branches that
-- add migrations in parallel never fight over `user_version`. New columns later go in a
-- numbered migration as usual.

-- One tree of text blocks and subpages. Craft sync is block to block on this table.
CREATE TABLE IF NOT EXISTS mc_node (
    id            TEXT PRIMARY KEY,
    space         TEXT NOT NULL CHECK (space IN ('viz', 'ver', 'newme')),
    parent_id     TEXT REFERENCES mc_node(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL DEFAULT 'text' CHECK (kind IN ('page', 'text', 'image', 'link', 'divider', 'opaque')),
    style         TEXT NOT NULL DEFAULT 'body',        -- Craft words: h1 h2 h3 h4 body caption bullet numbered card page
    text          TEXT NOT NULL DEFAULT '',            -- markdown as Craft stores it
    context       TEXT,                                -- the line under a card: what it is for (app side)
    url           TEXT,                                -- link blocks
    rank          TEXT NOT NULL DEFAULT 'n',           -- fractional index among siblings
    closed        INTEGER NOT NULL DEFAULT 0,          -- done or dropped (a Craft strike-through)
    asset_id      TEXT,
    origin        TEXT NOT NULL DEFAULT 'owner',       -- owner | nora | craft | import
    sync_scope    TEXT NOT NULL DEFAULT 'craft' CHECK (sync_scope IN ('craft', 'local')),
    sensitive     INTEGER NOT NULL DEFAULT 0,
    verified_at   TEXT,                                -- day the owner last said "still true"
    superseded_by TEXT,                                -- newer node that replaced this one
    craft_id      TEXT UNIQUE,                         -- Craft block id
    craft_base    TEXT,                                -- hash agreed at the last sync
    craft_seen_at TEXT,                                -- Craft lastModifiedAt at the last sync
    meta          TEXT NOT NULL DEFAULT '{}',
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    deleted_at    TEXT
);
CREATE INDEX IF NOT EXISTS mc_node_parent ON mc_node(parent_id, rank);
CREATE INDEX IF NOT EXISTS mc_node_space ON mc_node(space);

-- Which Craft document each space maps to (the sync allowlist). Empty until sync is turned on.
CREATE TABLE IF NOT EXISTS craft_link (
    space   TEXT PRIMARY KEY,
    doc_id  TEXT NOT NULL UNIQUE,
    polled_at TEXT
);

-- Images, stored outside the repo (paths::media_dir), keyed by content hash.
CREATE TABLE IF NOT EXISTS asset (
    id         TEXT PRIMARY KEY,
    sha256     TEXT NOT NULL UNIQUE,
    mime       TEXT NOT NULL,
    bytes      INTEGER NOT NULL,
    file       TEXT NOT NULL,           -- file name inside the media folder
    source_url TEXT,
    craft_url  TEXT,
    created_at TEXT NOT NULL
);

-- Boards: one canvas engine, three kinds. A viz board's content lives in mc_node.
CREATE TABLE IF NOT EXISTS board (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    kind            TEXT NOT NULL DEFAULT 'viz' CHECK (kind IN ('free', 'viz', 'project')),
    parent_board_id TEXT REFERENCES board(id) ON DELETE CASCADE,
    node_id         TEXT REFERENCES mc_node(id) ON DELETE SET NULL,   -- the page block (viz)
    accent          TEXT,                                             -- pillar hue name
    cover_asset     TEXT,
    layout          TEXT NOT NULL DEFAULT 'grid' CHECK (layout IN ('grid', 'canvas', 'story')),
    camera          TEXT,                                             -- {"x":..,"y":..,"z":..}
    project_kind    TEXT,
    project_id      TEXT,
    sensitive       INTEGER NOT NULL DEFAULT 0,
    archived        INTEGER NOT NULL DEFAULT 0,
    rank            TEXT NOT NULL DEFAULT 'n',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    deleted_at      TEXT
);

-- Layout of one card on a board. Replaces viz_tile. Content is referenced, never copied.
CREATE TABLE IF NOT EXISTS board_item (
    id         TEXT PRIMARY KEY,
    board_id   TEXT NOT NULL REFERENCES board(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('image', 'text', 'link', 'goal', 'board')),
    ref_kind   TEXT,               -- mc_node | mc_goal | board
    ref_id     TEXT,
    x          REAL NOT NULL DEFAULT 0,
    y          REAL NOT NULL DEFAULT 0,
    w          REAL NOT NULL DEFAULT 240,
    h          REAL NOT NULL DEFAULT 300,
    rot        REAL NOT NULL DEFAULT 0,
    z          INTEGER NOT NULL DEFAULT 0,
    color      TEXT,
    play_order INTEGER,
    sensitive  INTEGER NOT NULL DEFAULT 0,
    version    INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
);
CREATE INDEX IF NOT EXISTS board_item_board ON board_item(board_id);

-- A dream is a card in Visualization. Every dream carries its path.
CREATE TABLE IF NOT EXISTS dream (
    node_id         TEXT PRIMARY KEY REFERENCES mc_node(id) ON DELETE CASCADE,
    why             TEXT,
    outcome         TEXT,
    obstacle        TEXT,
    if_then         TEXT,
    first_step      TEXT,
    stage           TEXT NOT NULL DEFAULT 'seed' CHECK (stage IN ('seed', 'rooted', 'moving', 'reached', 'dropped')),
    closed_reason   TEXT,
    last_touched_at TEXT,
    review_every_days INTEGER NOT NULL DEFAULT 21
);

-- Version I want to be: goals by horizon; steps and substeps are the parent chain.
CREATE TABLE IF NOT EXISTS mc_goal (
    id            TEXT PRIMARY KEY,
    parent_id     TEXT REFERENCES mc_goal(id) ON DELETE CASCADE,
    node_id       TEXT REFERENCES mc_node(id) ON DELETE SET NULL,
    horizon       TEXT NOT NULL DEFAULT 'short' CHECK (horizon IN ('short', 'mid', 'long')),
    section       TEXT NOT NULL DEFAULT 'physical',   -- physical | mental | monetary | social | other
    title         TEXT NOT NULL,
    why           TEXT,
    dream_node_id TEXT,
    measure       TEXT NOT NULL DEFAULT 'done' CHECK (measure IN ('done', 'count', 'number', 'streak14')),
    target        REAL,
    current       REAL,
    unit          TEXT,
    due_on        TEXT,
    status        TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'moving', 'done', 'dropped', 'parked')),
    closed_reason TEXT,
    task_area     TEXT,      -- done tasks in this area count as progress
    habit_id      TEXT,      -- logs of this habit count as progress
    rank          TEXT NOT NULL DEFAULT 'n',
    origin        TEXT NOT NULL DEFAULT 'owner',
    verified_at   TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    deleted_at    TEXT
);
CREATE INDEX IF NOT EXISTS mc_goal_parent ON mc_goal(parent_id);

-- "I am someone who ..." statements, per section. Votes are evidence rows.
CREATE TABLE IF NOT EXISTS identity_statement (
    id            TEXT PRIMARY KEY,
    section       TEXT NOT NULL DEFAULT 'mental',
    text          TEXT NOT NULL,
    old_default   TEXT,
    status        TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('draft', 'active', 'retired')),
    node_id       TEXT,
    origin        TEXT NOT NULL DEFAULT 'owner',
    verified_at   TEXT,
    superseded_by TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

-- New Me: rules (usool), understanding, tricks; if-then when it has a trigger.
CREATE TABLE IF NOT EXISTS mc_rule (
    id             TEXT PRIMARY KEY,
    node_id        TEXT,
    kind           TEXT NOT NULL DEFAULT 'usool' CHECK (kind IN ('usool', 'understanding', 'in_doubt', 'trick', 'archived')),
    text           TEXT NOT NULL,
    trigger_when   TEXT,
    action_then    TEXT,
    origin_drop_id TEXT,
    status         TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('draft', 'active', 'retired')),
    check_mode     TEXT NOT NULL DEFAULT 'nightly' CHECK (check_mode IN ('nightly', 'weekly', 'none')),
    origin         TEXT NOT NULL DEFAULT 'owner',
    rank           TEXT NOT NULL DEFAULT 'n',
    verified_at    TEXT,
    superseded_by  TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS mc_rule_check (
    id      TEXT PRIMARY KEY,
    rule_id TEXT NOT NULL REFERENCES mc_rule(id) ON DELETE CASCADE,
    date    TEXT NOT NULL,
    kept    INTEGER NOT NULL,
    note    TEXT,
    by      TEXT NOT NULL,
    UNIQUE (rule_id, date)
);

-- The data drop: observations that may become understanding, then rules.
CREATE TABLE IF NOT EXISTS drop_entry (
    id      TEXT PRIMARY KEY,
    at      TEXT NOT NULL,
    kind    TEXT NOT NULL DEFAULT 'observation' CHECK (kind IN ('observation', 'pattern', 'learning', 'experiment', 'metric')),
    text    TEXT NOT NULL,
    data    TEXT NOT NULL DEFAULT '{}',
    sources TEXT NOT NULL DEFAULT '[]',
    status  TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'confirmed', 'rejected', 'became_rule')),
    by      TEXT NOT NULL DEFAULT 'owner',
    rule_id TEXT
);

-- One table links everything to everything: tasks, habits, journal -> goals, statements, rules, dreams.
CREATE TABLE IF NOT EXISTS evidence (
    id        TEXT PRIMARY KEY,
    at        TEXT NOT NULL,
    from_kind TEXT NOT NULL,     -- task | habit | journal | checkin | drop | manual
    from_id   TEXT NOT NULL,
    to_kind   TEXT NOT NULL,     -- goal | statement | rule | dream | drop
    to_id     TEXT NOT NULL,
    relation  TEXT NOT NULL DEFAULT 'advances',   -- advances | supports | contradicts | explains
    weight    REAL NOT NULL DEFAULT 1,
    note      TEXT,
    by        TEXT NOT NULL,
    UNIQUE (from_kind, from_id, to_kind, to_id)
);
CREATE INDEX IF NOT EXISTS evidence_to ON evidence(to_kind, to_id);

-- Changes Nora drafts. She raises them in conversation; applied only when he agrees there.
CREATE TABLE IF NOT EXISTS mc_proposal (
    id            TEXT PRIMARY KEY,
    journal_id    TEXT,
    target_kind   TEXT NOT NULL,
    target_id     TEXT,
    op            TEXT NOT NULL CHECK (op IN ('add', 'edit', 'close', 'retire', 'link')),
    payload       TEXT NOT NULL DEFAULT '{}',
    quote         TEXT,
    reason        TEXT,
    status        TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'raised', 'accepted', 'rejected', 'expired')),
    raised_at     TEXT,
    decided_at    TEXT,
    reject_reason TEXT,
    result_id     TEXT,
    by            TEXT NOT NULL,
    created_at    TEXT NOT NULL
);

-- Undo log for text changes (30 days).
CREATE TABLE IF NOT EXISTS mc_event (
    id          TEXT PRIMARY KEY,
    at          TEXT NOT NULL,
    actor       TEXT NOT NULL,
    target_kind TEXT NOT NULL,
    target_id   TEXT NOT NULL,
    before      TEXT,
    after       TEXT
);
