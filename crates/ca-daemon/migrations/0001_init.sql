-- 0001_init.sql — initial schema for ca-daemon's local state.
--
-- Foreign keys are enabled by db::open() via the connection options.
-- All timestamps are stored as ISO-8601 strings (no chrono dep yet).
-- Variant fields like ArchitectorState / TaskStatus are stored as JSON
-- text or snake_case strings, depending on whether they carry payload.

CREATE TABLE architectors (
    id            TEXT PRIMARY KEY,
    repo          TEXT NOT NULL,
    milestone_id  TEXT NOT NULL,
    issue_url     TEXT NOT NULL,
    state         TEXT NOT NULL,    -- JSON: ArchitectorState
    created_at    TEXT NOT NULL     -- ISO-8601
);

CREATE INDEX idx_architectors_repo_milestone
    ON architectors(repo, milestone_id);

CREATE TABLE tasks (
    id              TEXT PRIMARY KEY,                   -- e.g. M0-T1
    architector_id  TEXT NOT NULL
        REFERENCES architectors(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    deliverable     TEXT NOT NULL,
    blockers        TEXT NOT NULL,    -- JSON: Vec<String>
    status          TEXT NOT NULL,    -- TaskStatus, snake_case
    estimated_loc   INTEGER,
    created_at      TEXT NOT NULL
);

CREATE INDEX idx_tasks_architector ON tasks(architector_id);
CREATE INDEX idx_tasks_status      ON tasks(status);

CREATE TABLE dispatches (
    id            TEXT PRIMARY KEY,
    task_id       TEXT NOT NULL
        REFERENCES tasks(id) ON DELETE CASCADE,
    branch        TEXT NOT NULL,
    pr_url        TEXT,
    started_at    TEXT NOT NULL,
    completed_at  TEXT,
    phase         TEXT NOT NULL     -- coding | reviewing | drafted | merged | dropped | ...
);

CREATE INDEX idx_dispatches_task  ON dispatches(task_id);
CREATE INDEX idx_dispatches_phase ON dispatches(phase);

CREATE TABLE reviews (
    id           TEXT PRIMARY KEY,
    dispatch_id  TEXT NOT NULL
        REFERENCES dispatches(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL,    -- security | bugs | quality | critique
    summary      TEXT,
    body         TEXT NOT NULL,    -- JSON: ReviewResult or CritiqueResult
    created_at   TEXT NOT NULL
);

CREATE INDEX idx_reviews_dispatch ON reviews(dispatch_id);
CREATE INDEX idx_reviews_kind     ON reviews(kind);
