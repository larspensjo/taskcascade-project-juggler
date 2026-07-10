# TaskCascade — High-Level Design

TaskCascade is a single-developer coordination tool built around one ordered work stack. It intentionally omits tickets, estimates, dates, and workflow states: a task is either active at a list position or completed in the archive.

## Architecture

| Layer | Choice | Responsibility |
| --- | --- | --- |
| Local backend | Rust + axum | REST API, ordering invariant, persistence boundary |
| Storage | SQLite | Projects, active/archive task records, UI preferences |
| Web UI | Vite + React + TypeScript | Keyboard-first task list, editor, Markdown preview |
| Markdown | `react-markdown` + GFM | Safe rendering of descriptions and scratchpads |

In development, Vite proxies `/api` requests to the loopback Rust server. In a built application, the Rust server can serve `frontend/dist` directly. The SQLite database defaults to `Documents/TaskCascade/taskcascade.sqlite`; `TASKCASCADE_DATA_DIR` supplies a different location for development or testing.

## Data model

`projects` contains user-configurable names. `tasks` stores title, description, scratchpad, project, active-list position, and timestamps. A null `completed_at` means active; a populated value makes the record archive-only. `preferences` currently stores the last project filter and can safely grow for other local UI preferences.

## Interaction model

The active list is globally ordered. Drag-and-drop and keyboard moves issue relative moves, then persist the full resulting order atomically. Editing writes after a short idle delay, so there is no Save button. Search is server-side and covers title, description, scratchpad, active tasks, and archive.
