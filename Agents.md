# Repo Instructions

## Workflow

- The repository is a Cargo workspace (`backend/`, `desktop/`) with one `target/`. Run Cargo commands from the repository root.
- For backend-only changes, run `cargo test -p taskcascade-backend` and `cargo clippy -p taskcascade-backend --all-targets -- -D warnings`. For changes to `desktop/` or a workspace manifest, run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`. Finish with `cargo fmt --all`.
- For desktop or workspace-manifest changes, also run `cargo clippy -p taskcascade-desktop --release --all-targets -- -D warnings`; build `frontend/dist` first with `npm run build` from `frontend/`.
- For frontend changes, run `npm test`, `npm run check` and `npm run fmt` from `frontend/`.
- Use `scripts/start.ps1` to open the development window on scratch data in `.local/dev-data`. Only Lars runs `scripts/install.ps1` to update his daily app and real Start-menu shortcut; agent rehearsals must use a scratch `-InstallDir` and `-NoShortcut`.
- Never point development, test or rehearsal runs at `%LOCALAPPDATA%\TaskCascade`; it holds Lars's real tasks. Release builds default to the real data. Never launch a release executable without `TASKCASCADE_DATA_DIR` pointing at scratch; use `scripts/start.ps1 -Release`. Do not use the pre-migration `.local/data` as a fixture. Script rehearsals in `scripts/tests/` use synthetic fixtures under a fresh temporary root passed with `-Root`; never install to the real location during a rehearsal.
- The UI follows the calm, compact dark-token approach recorded in `docs/VisualDesign.DarkTheme.md`; use CSS tokens rather than inline colours.

## Architecture

- The application is a local-first desktop app: SQLite is the data of record; the Tauri shell in `desktop/` serves the UI and the axum API in-process through one custom URI scheme — no TCP listener, no browser. The frontend uses relative `/api` requests and no Tauri JavaScript API.
- The `backend` crate is a library (API, SQL, domain). Shell, path, window and WebView2 concerns live in `desktop/`.
- Preserve the ordered active list invariant: task position is the only priority. Completed tasks are archived and must remain immutable from the UI/API.
- Keep pure ordering or search derivation in the domain layer so it remains unit-testable; keep HTTP and SQL code in the app layer.
- Keep React input flow predictable: user input → local draft → API mutation → refreshed application state → render. Isolate API calls in `frontend/src/api.ts`.
- Task drafts are owned by the save coordinator (`frontend/src/saveCoordinator.ts`). Completing, deleting and reordering active tasks (by drag or keyboard), and window close, wait for a confirmed save; failed drafts survive and offer Retry. Closing also offers the explicit choice **Close and discard unsaved changes**.
- Markdown is content, not HTML. Do not enable raw HTML rendering without an explicit security decision.

## Decisions and documentation

- `docs/DecisionLog.md` is the durable record of non-obvious commitments. Add concise, new entries at the end; do not rewrite old entries.
- Plans and design notes may be placed under `docs/` when they express a lasting decision or contract. Keep implementation milestones out of durable docs.

## Testing

- Add focused regression tests for bugs where practical.
- Prefer public behavior, pure ordering logic, and API contracts over implementation-detail tests.
