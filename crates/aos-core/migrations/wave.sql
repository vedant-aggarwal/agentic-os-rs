-- Wave (urge help). Applied on every open with IF NOT EXISTS, like mission.sql, so parallel
-- branches never fight over `user_version`.

CREATE TABLE IF NOT EXISTS urge (
    id           TEXT PRIMARY KEY,
    date         TEXT NOT NULL,                 -- local day it started
    hour         INTEGER NOT NULL,              -- local hour it started, 0..23
    started_at   TEXT NOT NULL,
    ends_at      TEXT NOT NULL,                 -- the ten-minute timer
    finished_at  TEXT,
    intensity    INTEGER CHECK (intensity BETWEEN 0 AND 10),
    place        TEXT,                          -- bed | bathroom | desk | elsewhere
    alone        INTEGER,
    feelings     TEXT NOT NULL DEFAULT '[]',    -- JSON array of chips
    trigger_kind TEXT,                          -- what set it off
    action       TEXT,                          -- the reset action he picked
    lie          TEXT,                          -- what his brain said
    truth        TEXT,                          -- what he replaced it with
    outcome      TEXT CHECK (outcome IN ('beaten', 'strong', 'slipped')),
    note         TEXT,
    steps        TEXT NOT NULL DEFAULT '[]',    -- steps he went through
    xp           INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS urge_date ON urge (date, started_at);

CREATE TABLE IF NOT EXISTS wave_setting (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- The night guard: one row per night Nora asked "phone docked outside the bed?".
CREATE TABLE IF NOT EXISTS wave_guard (
    date     TEXT PRIMARY KEY,
    asked_at TEXT NOT NULL,
    docked   INTEGER,
    note     TEXT
);
