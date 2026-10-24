# TaskCascade — High-Level Design

TaskCascade is a single-developer coordination tool built around one ordered work stack. It intentionally omits tickets, estimates, dates, and configurable workflow states: tasks are active at a list position, completed in the archive, or soft-deleted in the trash.

## Architecture

| Layer | Choice | Responsibility |
| --- | --- | --- |
| Desktop shell | Rust + Tauri 2 on WebView2 | Window, scheme bridge, frontend assets, content-security policy, data location, single instance, close handshake and bounded shutdown |
| Local backend | Rust + axum library | REST API, ordering invariant, persistence boundary |
| Storage | SQLite | Projects, active/archive task records, UI preferences |
| UI | Vite + React + TypeScript | Keyboard-first task list, editor, Markdown preview; built assets embedded in release builds |
| Markdown | `react-markdown` + GFM | Safe rendering of descriptions and scratchpads |

The `taskcascade-desktop` executable loads `taskcascade://localhost/`. Its asynchronous scheme handler passes requests directly into the axum router on a shell-owned Tokio runtime; no TCP listener or browser is used. WebView2 presents the page and API at one origin, `http://taskcascade.localhost`, so the frontend keeps relative `/api` requests. The `taskcascade-backend` package is a library; the shell owns static assets and reads `frontend/dist` from disk in debug builds. Release builds embed those assets. Interface changes require rebuilding and reloading a debug window, or rebuilding and reopening a release window.

The shell emits the content-security policy on responses. Scripts, styles, fonts and API requests are restricted to the app's origin, with inline styles allowed. Inline scripts and `data:` sources are forbidden; Vite uses `assetsInlineLimit: 0`. Images may come from the app's origin or remote https sources; plain-http remote images do not render. Markdown is rendered without raw HTML. The page has no Tauri JavaScript API or typed commands. If a capability file is ever added, it must not grant opener permissions such as `opener:default`, which would expose a URL-opening command to page script.

### Data and window identity

The release default is `%LOCALAPPDATA%\TaskCascade\taskcascade.sqlite`; debug builds default to the repository's `.local/dev-data/taskcascade.sqlite`. `TASKCASCADE_DATA_DIR` overrides the directory and must be absolute. Location resolution requires `LOCALAPPDATA` in both profiles. Existing directories are compared by filesystem identity; missing paths use a canonical existing ancestor and normalized remainder. Directories containing or nested inside the real directory, and a different directory whose database aliases the real file, are refused.

The same resolved `is_real` result selects identity and creation policy. The real location uses identifier `se.pensjo.taskcascade` and title **TaskCascade**. Every other location shares `se.pensjo.taskcascade.dev` and title **TaskCascade (dev)**, including release builds on scratch data. Single instance is scoped to those two identities, so different scratch directories still share one development instance. A second launch shows, unminimizes and focuses the existing window, before opening SQLite. The identifier also scopes saved window state and the WebView2 profile. Window state is restored before showing the initially hidden window; saved visibility is excluded.

A missing database is created only with `TASKCASCADE_CREATE_DATABASE=1`, or implicitly for a debug build at a non-real location. The real location always requires explicit creation. Startup failures use a native dialog naming the database and available log path. Best-effort diagnostics append to `<data directory>\logs\taskcascade-desktop.log`, with one rotation at startup when the existing log reaches 1 MiB. They record requested and canonical database paths, identity, an eval self-test, close outcomes and shutdown results.

The daily app is a separate installed release executable under `%LOCALAPPDATA%\Programs\TaskCascade`, updated by `scripts/install.ps1` while that copy is closed. Its Start-menu shortcut is independent of repository rebuilds. `scripts/start.ps1` builds and opens the development window on `.local/dev-data` with creation explicitly enabled. Both scripts refuse a remaining pre-migration `.local/data` database or sidecar; `scripts/migrate-data.ps1` moves and verifies that data first. The installed and development windows can run together.

### Navigation and native controls

Only the app's exact root document is allowed to navigate within the window. External `http` and `https` links open in the system browser; `mailto` opens the system mail handler. Other targets, including non-root relative paths, `file:` and `javascript:`, are blocked. A new webview window is always denied, while approved external targets still open through the system handler in Rust. The opener plugin's JavaScript link interception is disabled.

Release builds disable WebView2's browser accelerator keys and keep its default context menu only on editable text or a text selection; other targets show no menu. Devtools are enabled only in debug builds. Native drag handling is disabled so HTML5 task dragging reaches the interface.

## Data model

`projects` contains user-configurable names. `tasks` stores title, description, scratchpad, project, active-list position, and timestamps. A populated `deleted_at` puts a task in the trash; otherwise a null `completed_at` means active and a populated value means archived. Archived fields are read-only, and deleted tasks are excluded from search and mutations except undelete. `preferences` currently stores the last project filter and can safely grow for other local UI preferences.

## Interaction model

The active list is globally ordered. Drag-and-drop and keyboard moves issue relative moves, then persist the full resulting order atomically. Search runs in the backend and covers title, description, scratchpad, active tasks, and archive.

Edits are kept above the keyed editor by `frontend/src/saveCoordinator.ts`, which writes after a 450 ms idle delay, one request per task at a time. Newer edits coalesce, and an older response cannot clear a newer draft. Switching away starts the save immediately. Completing, deleting and reordering active tasks (by drag or keyboard) wait for a confirmed save. Failed drafts survive editor unmount and keep the error with Retry.

On close, the shell evaluates a script in the page that first posts `/desktop/close-ack`, then invokes `window.taskcascadeBeforeClose`. The page freezes edits and mutations, flushes all drafts and waits for in-flight API requests, bounded at 10 seconds, then reports through `/desktop/close-ready`. A reported failure restores editing and keeps the window open with **Retry and close**, **Close and discard unsaved changes**, and **Keep editing**. Explicit discard drops the drafts and reports that choice to the shell.

If no acknowledgement arrives within 3 seconds, or the page reports that its close hook is absent, the shell closes without save confirmation. If an acknowledged page stalls for 20 seconds, its hook throws, or evaluating the handshake fails, a native Yes/No question asks whether to close anyway; No keeps the window open. An explicit save failure disarms the close timers. These unavailable-page paths cannot guarantee preservation of drafts. Abrupt termination (task kill, Ctrl+C, operating-system shutdown) can also lose a pending edit.

Once closing is decided, one 5-second budget covers shutdown: new API requests receive 503, in-flight requests drain for up to 2 seconds, the SQLite pool closes with the remaining budget less a 500 ms runtime reserve, and runtime teardown uses what remains. Shutdown is idempotent and logs any exhausted stage. A confirmed save has already committed; exhausting shutdown does not undo it.
