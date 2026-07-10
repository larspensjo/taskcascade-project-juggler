CREATE TABLE projects (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL COLLATE NOCASE UNIQUE,
    created_at TEXT NOT NULL
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    scratchpad TEXT NOT NULL DEFAULT '',
    project_id TEXT NOT NULL REFERENCES projects(id),
    position INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    modified_at TEXT NOT NULL,
    completed_at TEXT
);

CREATE INDEX tasks_active_position ON tasks(completed_at, position);
CREATE INDEX tasks_project ON tasks(project_id);

CREATE TABLE preferences (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
