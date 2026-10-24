# Decision Log

Purpose: durable record of deliberate commitments — the source of truth for what the project has agreed to do going forward.

## How to use

- One entry per decision; decisions are commitments, not work summaries.
- Add new entries at the end. A reversal is a new entry that refers to the earlier decision.
- Record architecture, project-wide conventions, scope boundaries, and results promoted from an investigation.
- Do not amend an already committed entry; preserve history.

## Entry template

```markdown
## YYYY-MM-DD — Decision title

Decision: Present-tense commitment.
Context: Why this decision was needed.
Consequences: What it constrains or enables going forward.
```

## 2026-07-10 — Local SQLite task stack

Decision: The MVP is a single-user Rust (axum) and TypeScript web application backed by one local SQLite file, with no remote service or account model.
Context: A personal coordination tool needs immediate persistence, easy backup, and zero configuration rather than collaboration infrastructure.
Consequences: The backend listens on loopback by default, the database is the source of truth for tasks, projects, archive records, and user preferences, and backup is copying one file.

## 2026-07-10 — Position is the sole active-task priority

Decision: Every active task shares one global ordered list; its position is the only priority model. Project filtering changes visibility only, never the stored order.
Context: Numeric priority, status, and separate stacks add interaction without helping the MVP answer “what should I do next?”.
Consequences: Reorder operations are relative moves inside the global list. A filtered view may be used to initiate a move, but hidden tasks retain their relative placement.

## 2026-07-10 — Completion creates a read-only archive

Decision: Completing a task timestamps it and removes it from the active stack; the archived task remains searchable but cannot be edited or restored in the MVP.
Context: Completion should be fast while preserving useful history and scratchpad context.
Consequences: The data model retains all task fields and timestamps. Restore and archive mutations remain explicitly post-MVP work.

## 2026-07-11 — Archived tasks can be restored to the stack

Decision: An archived task can be restored to the top of the active stack. This supersedes the no-restore portion of the 2026-07-10 "Completion creates a read-only archive" decision; the rest of that decision stands.
Context: Completed tasks sometimes need to come back — a piece of work turns out to be unfinished, or is repeated. Viewing the archive already existed; only the restore action was missing.
Consequences: Restore is a lifecycle action, not field editing — archived task content stays read-only. Restoring clears `completed_at` (an active task has no completion timestamp), so no completion history is retained across a restore; tasks that remain in the archive keep all their timestamps. Retaining completion history is still future work ("Task history").

## 2026-07-13 — Drag-and-drop replaces archive/restore buttons; soft-delete trash

Archiving, restoring, deleting, and undeleting are drag gestures onto sidebar
drop targets (Archive, Deleted, All tasks); the per-row ✓ button and the
editor's Complete/Restore buttons are removed (the Delete key remains the
keyboard path for completing). Tasks gain a third state via `deleted_at`;
deleting clears `completed_at` so the undelete drop target chooses the
destination ("All tasks" → stack top, "Archive" → fresh completion
timestamp). Deleted tasks are excluded from search and all mutations except
undelete. No purge mechanism — the trash only grows until a future decision.
This softens the earlier "archive is immutable" stance: archived tasks can
now be moved to the trash. Spec:
`docs/superpowers/specs/2026-07-13-drag-drop-archive-trash-design.md`.

## 2026-10-23 — Closing the desktop window never loses an edit

Decision: Closing the desktop window never discards an unsaved edit unless the user explicitly picks "Close and discard unsaved changes". Edits save after a short debounce; complete, delete and drag actions on an active task wait for a confirmed save; closing runs a save-and-confirm handshake with the shell and keeps the window open, with Retry, when a save fails.
Context: The desktop window can close at any moment (X, Alt+F4, taskbar), and a pending edit used to live only in the editor's local state.
Consequences: Once closing is decided, the shell stops accepting API calls (503) and drains in-flight requests, all inside the single 5-second shutdown deadline. If the page cannot answer, a native Yes/No question decides. Plan: `docs/Plans/Plan.desktop-window.md` Phase 7.

## 2026-10-24 — TaskCascade is a desktop application; browser support is removed

Decision: TaskCascade runs as a native desktop window (Tauri 2 on WebView2). The window loads the interface and API through one custom URI scheme whose handler feeds requests into the axum router in-process; no TaskCascade process opens a TCP listener or port, and there is no browser run path. Page and API share one origin, so the frontend keeps relative `/api` requests and uses no Tauri JavaScript API or typed commands. The `taskcascade-backend` package is a library (API, SQL, domain); `taskcascade-desktop` is the application executable and owns frontend assets and the content-security policy.
Context: A stand-alone window was wanted instead of a browser tab plus a background server. This supersedes the "web application" and "backend listens on loopback" parts of the 2026-07-10 "Local SQLite task stack" entry — its single-user, local-SQLite and copy-one-file-to-back-up commitments stand — and replaces the never-implemented single-binary browser packaging design (git 14f5e5f), which had chosen browser distribution over Tauri. Keeping two run paths was rejected as two shells to maintain for one user.
Consequences: Interface changes require rebuilding and reloading a debug window, or rebuilding and reopening a release window; there is no live reload. The permissive CORS layer and Vite `/api` proxy are gone. Nothing outside the process can reach the API, so an external client, including the parked agent-access plan, needs a separate future decision. The policy forbids inline scripts and `data:` sources; frontend assets must not be inlined by the bundler. Inline styles remain allowed.

## 2026-10-24 — Real tasks live in local application data; the installed copy and the development launcher are separate

Decision: Lars's tasks live in `%LOCALAPPDATA%\TaskCascade\taskcascade.sqlite`, outside the repository and synced folders. The daily app is a self-contained release executable with embedded frontend, installed in `%LOCALAPPDATA%\Programs\TaskCascade` by `scripts/install.ps1` with a Start-menu shortcut. Repository rebuilds never change it. `scripts/start.ps1` opens a development window on `.local/dev-data` scratch data. The resolved filesystem identity of the data directory determines both identity and creation policy: the real location uses `se.pensjo.taskcascade` titled "TaskCascade"; every other location shares `se.pensjo.taskcascade.dev` titled "TaskCascade (dev)". Directories nested in or containing the real directory, and databases aliasing the real file from another directory, are refused. Each identity permits one instance; a second launch focuses the existing window before opening SQLite. A missing real database is never created implicitly: creation requires `TASKCASCADE_CREATE_DATABASE=1` (or `install.ps1 -InitDatabase`). Debug builds may implicitly create a missing non-real database; release builds require the opt-in even on scratch data.
Context: Real tasks had accumulated in repository scratch data, where development and cleanups could touch them; a verified one-time migration moved them. Running the daily app from the build tree was rejected because it locks rebuilds and exposes daily use to unfinished changes. A synced Documents folder risks corruption of live SQLite files and sidecars. Separate windows on the same database can overwrite notes from stale views; path-string comparison would let aliases of the real directory evade the real identity.
Consequences: Close the installed copy and re-run the installer to update it. The current launcher and installer both refuse a pre-migration `.local/data` database or sidecar and point to `scripts/migrate-data.ps1`. The launcher always selects scratch data and explicitly enables creation. `TASKCASCADE_DATA_DIR` must be absolute; release defaults to the real location and debug to scratch, while identity follows the resolved location in either profile. All scratch locations share the development instance scope, window state and WebView2 profile. Startup diagnostics name the requested and canonical database paths; failure dialogs name the database and available log path. The data default lives in `desktop/src/data_location.rs` and is repeated in scripts, which must stay aligned. Script rehearsals use synthetic fixtures under fresh temporary roots. Backups are manual copies while the app is closed; automated backups remain future work.

## 2026-10-24 — Inside the window: links leave, and edits are confirmed before they can be lost

Decision: The window stays on TaskCascade's root document. The shell opens external `http` and `https` links in the system browser and `mailto` links in the system mail handler; other navigation, including non-root relative links, `file:` and `javascript:`, is blocked. New webview windows are always denied, while approved external links still open through the system handler. Remote note images render only over https. Release builds disable WebView2's browser accelerator keys and keep its default context menu only on editable text and selections; devtools are debug-only. A save coordinator owns task drafts, serializes writes per task and preserves failed drafts with the error and Retry. Completing, deleting and reordering an active task (by drag or keyboard) wait for the latest draft's confirmed save. Closing freezes editing and mutations, confirms pending saves and keeps the window open on reported failure, offering **Retry and close**, **Close and discard unsaved changes**, and **Keep editing**. Explicit discard is the deliberate way to drop a failed draft.
Context: This extends the 2026-10-23 "Closing the desktop window never loses an edit" entry and makes its unavailable-page exceptions precise; the plan cited there is retired and remains in git history at 042f1c7. The shell must handle navigation and closing itself; following a link must not replace the app. A close or lifecycle action during the save delay, or after a rejected save such as a blank title, must not silently drop a draft. A finished request is insufficient: persistence must have succeeded.
Consequences: The shell/page contract is `window.taskcascadeBeforeClose`, `/desktop/close-ack` and `/desktop/close-ready`. The page's save-and-settle wait is bounded. No acknowledgement within a few seconds, or a report that the hook is absent, closes without save confirmation. An acknowledged page that stalls beyond a longer deadline, a thrown hook or failed script evaluation asks the native Yes/No "Close anyway?" question; No keeps the window open. An explicit save failure disarms the timers and preserves the draft. These unavailable-page paths qualify the older entry's general no-loss wording; drafts cannot be guaranteed there. After closing is decided, a single 5-second budget stops API admission (503), bounds request draining, closes the pool while reserving time for runtime teardown and tears down the runtime. Confirmed edits have already committed. Plain-http remote images no longer render. Abrupt process termination, Ctrl+C or operating-system shutdown can still lose a pending edit; committed data is unaffected.
