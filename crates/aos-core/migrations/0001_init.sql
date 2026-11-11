-- Agentic OS schema v1.
-- Ids are short random text ids so offline clients can create rows without asking the server.
-- Timestamps are RFC 3339 UTC strings. Dates (`date`, `due`) are local calendar days, YYYY-MM-DD.

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Plan ---------------------------------------------------------------------

CREATE TABLE pillar (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    sort       INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE area (
    id         TEXT PRIMARY KEY,
    pillar_id  TEXT REFERENCES pillar(id) ON DELETE SET NULL,
    name       TEXT NOT NULL,
    color      TEXT,
    sort       INTEGER NOT NULL DEFAULT 0,
    archived   INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE project (
    id         TEXT PRIMARY KEY,
    area_id    TEXT REFERENCES area(id) ON DELETE SET NULL,
    name       TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE task (
    id             TEXT PRIMARY KEY,
    parent_id      TEXT REFERENCES task(id) ON DELETE CASCADE,
    area_id        TEXT REFERENCES area(id) ON DELETE SET NULL,
    project_id     TEXT REFERENCES project(id) ON DELETE SET NULL,
    title          TEXT NOT NULL,
    notes          TEXT,
    status         TEXT NOT NULL DEFAULT 'open'
                   CHECK (status IN ('open', 'doing', 'done', 'skipped', 'dropped')),
    due            TEXT,
    scheduled      TEXT,
    xp_weight      INTEGER NOT NULL DEFAULT 1 CHECK (xp_weight BETWEEN 0 AND 10),
    skip_reason    TEXT,
    reason_genuine INTEGER,
    evidence       TEXT,
    sort           REAL NOT NULL DEFAULT 0,
    source         TEXT NOT NULL DEFAULT 'app',
    external_id    TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    closed_at      TEXT
);
CREATE INDEX task_parent ON task(parent_id);
CREATE INDEX task_status ON task(status);
CREATE INDEX task_due ON task(due);
CREATE UNIQUE INDEX task_external ON task(source, external_id) WHERE external_id IS NOT NULL;

-- Habits -------------------------------------------------------------------

CREATE TABLE habit (
    id         TEXT PRIMARY KEY,
    area_id    TEXT REFERENCES area(id) ON DELETE SET NULL,
    name       TEXT NOT NULL,
    cue        TEXT,
    normal     TEXT,
    floor      TEXT,
    cadence    TEXT NOT NULL DEFAULT 'daily',
    xp         INTEGER NOT NULL DEFAULT 5,
    anchor     INTEGER NOT NULL DEFAULT 0,
    archived   INTEGER NOT NULL DEFAULT 0,
    start_date TEXT NOT NULL,
    sort       INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE habit_log (
    id         TEXT PRIMARY KEY,
    habit_id   TEXT NOT NULL REFERENCES habit(id) ON DELETE CASCADE,
    date       TEXT NOT NULL,
    status     TEXT NOT NULL CHECK (status IN ('done', 'floor', 'missed', 'excused')),
    note       TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (habit_id, date)
);

-- Game ---------------------------------------------------------------------

-- Append-only. Corrections are new rows with a negative amount.
CREATE TABLE xp_event (
    id         TEXT PRIMARY KEY,
    source     TEXT NOT NULL,
    source_id  TEXT,
    area_id    TEXT REFERENCES area(id) ON DELETE SET NULL,
    amount     INTEGER NOT NULL,
    note       TEXT,
    date       TEXT NOT NULL,
    at         TEXT NOT NULL
);
CREATE INDEX xp_event_source ON xp_event(source, source_id);
CREATE INDEX xp_event_date ON xp_event(date);

-- Inner life ---------------------------------------------------------------

CREATE TABLE mood (
    id           TEXT PRIMARY KEY,
    date         TEXT NOT NULL,
    at           TEXT NOT NULL,
    valence      REAL NOT NULL CHECK (valence BETWEEN -1.0 AND 1.0),
    energy       INTEGER CHECK (energy BETWEEN 1 AND 5),
    labels       TEXT NOT NULL DEFAULT '[]',
    reasons      TEXT NOT NULL DEFAULT '[]',
    note         TEXT,
    healthkit_id TEXT,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
CREATE INDEX mood_date ON mood(date);

-- `text` holds his words exactly as given. Nothing in the app rewrites it.
CREATE TABLE journal_entry (
    id         TEXT PRIMARY KEY,
    date       TEXT NOT NULL,
    at         TEXT NOT NULL,
    kind       TEXT NOT NULL DEFAULT 'journal',
    text       TEXT NOT NULL,
    nora_read  TEXT,
    source     TEXT NOT NULL DEFAULT 'app',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX journal_date ON journal_entry(date);

-- Alarms -------------------------------------------------------------------

CREATE TABLE alarm (
    id         TEXT PRIMARY KEY,
    at         TEXT NOT NULL,
    label      TEXT NOT NULL,
    kind       TEXT NOT NULL DEFAULT 'alarm' CHECK (kind IN ('alarm', 'reminder')),
    status     TEXT NOT NULL DEFAULT 'scheduled' CHECK (status IN ('scheduled', 'fired', 'cancelled')),
    task_id    TEXT REFERENCES task(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX alarm_at ON alarm(status, at);

-- Sync and auth ------------------------------------------------------------

-- Every write to a synced table adds a row here. Clients pull `version > since`.
CREATE TABLE change_log (
    version INTEGER PRIMARY KEY AUTOINCREMENT,
    entity  TEXT NOT NULL,
    row_id  TEXT NOT NULL,
    op      TEXT NOT NULL CHECK (op IN ('upsert', 'delete')),
    at      TEXT NOT NULL,
    actor   TEXT
);
CREATE INDEX change_log_row ON change_log(entity, row_id);

CREATE TABLE device (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    platform     TEXT,
    token_hash   TEXT NOT NULL UNIQUE,
    created_at   TEXT NOT NULL,
    last_seen_at TEXT,
    revoked_at   TEXT
);

CREATE TABLE pairing_code (
    id         TEXT PRIMARY KEY,
    code_hash  TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    attempts   INTEGER NOT NULL DEFAULT 0,
    used_at    TEXT,
    device_id  TEXT REFERENCES device(id) ON DELETE SET NULL
);

-- Outbox ops already applied, so a client can resend safely.
CREATE TABLE applied_op (
    op_id     TEXT PRIMARY KEY,
    device_id TEXT,
    tool      TEXT NOT NULL,
    result    TEXT NOT NULL,
    at        TEXT NOT NULL
);

-- Goals ---------------------------------------------------------------------

-- A concrete thing to earn or reach, usually one per pillar (a laptop, a motorbike...).
-- Money is integer paise (1 rupee = 100 paise) so sums are exact.
CREATE TABLE goal (
    id           TEXT PRIMARY KEY,
    pillar_id    TEXT REFERENCES pillar(id) ON DELETE SET NULL,
    area_id      TEXT REFERENCES area(id) ON DELETE SET NULL,
    name         TEXT NOT NULL,
    specs        TEXT NOT NULL DEFAULT '[]',   -- JSON list of specs / features
    why          TEXT,                         -- the emotional reason, his words
    images       TEXT NOT NULL DEFAULT '[]',   -- JSON list of image paths, URLs or model refs
    target_paise INTEGER CHECK (target_paise IS NULL OR target_paise >= 0),
    earned_paise INTEGER NOT NULL DEFAULT 0,
    progress     REAL CHECK (progress IS NULL OR progress BETWEEN 0.0 AND 1.0), -- for goals without a price
    status       TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'reached', 'dropped')),
    sort         INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    reached_at   TEXT
);
