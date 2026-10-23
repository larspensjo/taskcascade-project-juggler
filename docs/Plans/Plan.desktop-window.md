# TaskCascade as a Stand-alone Desktop Window (Tauri) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan phase-by-phase. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lars's daily TaskCascade is an installed copy started from a
Start-menu shortcut. It opens his real tasks at
`%LOCALAPPDATA%\TaskCascade\taskcascade.sqlite`, moved there once from the
repository's `.local/data`. `scripts/start.ps1` opens a separate development
window on scratch data. Both are native windows running the full app. Closing a
window stops that copy without losing the last edit. The one exception is the
user explicitly choosing "close and discard unsaved changes" after a save has
failed. Nothing opens in a browser, no process opens a TCP listener, and the
documentation describes only the desktop app.

**Architecture:** A new `desktop/` crate, a Tauri 2 shell on WebView2, is the
only executable. It registers one custom URI scheme (`taskcascade`) whose
asynchronous handler feeds every webview request into an in-process axum
router. That router runs on a tokio runtime the shell builds itself, and it is
the backend's existing `/api` router plus a desktop-owned static-asset handler
for the built frontend.

Page and API share one origin, so `frontend/src/api.ts` keeps its relative
`fetch("/api/...")`. There is no Tauri JavaScript API and no typed command. The
frontend is read from `frontend/dist` on disk in debug builds and embedded in
the release binary. The `backend/` crate becomes a library: API handlers, SQL,
migrations and domain logic. Its loopback server, CORS layer and static serving
are removed together with the Vite `/api` proxy.

Which database a window opens is resolved once at startup, against canonical
filesystem identities, and that one result decides the window's identity
(Tauri identifier, title, single-instance scope, window-state file, WebView2
profile) and whether a missing database may be created. Release builds default
to the real location and never create a missing real database implicitly.
Debug builds default to a repository scratch directory.

Edits are owned by a save coordinator that sits above the editor, serializes
saves per task, keeps failed drafts and gates lifecycle actions and window
close on confirmed persistence. Closing runs a handshake with the page and
then a single bounded shutdown.

**Tech Stack:** Rust (Cargo workspace: `backend`, `desktop`), Tauri 2 with
`tauri-plugin-window-state`, `tauri-plugin-single-instance` and
`tauri-plugin-opener` (Rust API only), `rust-embed` (debug reads from disk,
release embeds), `same-file` (directory and file identity), `windows-sys`
(`MessageBoxW`), and `webview2-com` only if the resolved Tauri version lacks
builder options for the WebView2 settings in Phase 6. axum 0.8 and sqlx 0.8
(SQLite) are unchanged. The frontend stays React 19, TypeScript and Vite, and
gains `vitest` for the save-coordinator and close-flow tests.

**Design sources:** `C:\Users\larsp\src\TickerTapeTallyBoard` (TTTB) is the
reference implementation: its `desktop/` crate, `scripts/start.ps1`,
`scripts/migrate-ledger.ps1`, and its decision-log entries from 2026-09-13 to
2026-09-20. Those entries cover the application-owned ledger location, the
Cargo workspace, workspace checks that include the desktop crate, in-process
transport over one custom URI scheme, absolute path overrides,
working-directory independence, one icon from a vector master, and images only
from served files.

This plan adapts TTTB rather than copying it. TaskCascade removes the web
server instead of keeping it, embeds the frontend in the release build,
separates an installed copy from a development launcher, enforces a single
instance per copy, migrates the data, guarantees confirmed saves on close,
intercepts links, and allows remote https images.

The review `docs/Review.desktop-window.md` (2026-10-22) is folded into this
revision. This repository's `docs/DecisionLog.md` entries from 2026-07-10
(local SQLite, position-is-priority, archive) and 2026-07-13 (drag-and-drop
and trash) still apply. The durable record of this work is new decision-log
entries plus updates to `docs/Design.HighLevel.md`,
`docs/RequirementSpecification.md`, `README.md` and `Agents.md` (final phase).

---

## Global Constraints

- **Lars's real data is never a test fixture.**
  - No automated test, agent run, rehearsal or by-eye check other than Lars's
    own steps marked **Lars** may open, read, copy or write
    `%LOCALAPPDATA%\TaskCascade\taskcascade.sqlite` (or its sidecars) or the
    pre-migration `.local/data/taskcascade.sqlite`. Only filesystem existence
    checks by the guards in the scripts are exempt.
  - Rehearsals of the migration, the interim launcher and the installer run on
    synthetic fixtures generated under a temporary root. Each rehearsal script
    refuses to run if any path it is given resolves inside either real-data
    location.
  - Agents never pass `-RealData` or `-CreateRealDataSnapshot` with default
    roots. They never point `TASKCASCADE_DATA_DIR` at the real location, and
    never launch a release `taskcascade-desktop.exe` without
    `TASKCASCADE_DATA_DIR` set to a scratch directory.
  - Backend tests use `sqlite::memory:` or `tempfile` directories.
- **Leave alone.**
  - `%LOCALAPPDATA%\TaskCascade\instances\` (written by the parked agent CLI
    branch).
  - The stale `%USERPROFILE%\Documents\TaskCascade\` copy (reported, never
    modified).
  - Lars's real Start-menu shortcut, which only the real installation in
    Phase 8 may create.
  - The `feature/agent-cli` branch: do not merge, rebase or depend on it.
  - `docs/Plans/Plan.agent-cli-access.md`: do not redesign it. The final phase
    only adds a one-line note.
- **Gates.**
  - Phase 1 runs backend commands from `backend/` as `Agents.md` prescribes
    today: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
    `cargo fmt`.
  - From Phase 2 on, Cargo runs from the repository root:
    - backend-only changes: `cargo test -p taskcascade-backend`,
      `cargo clippy -p taskcascade-backend --all-targets -- -D warnings`,
      `cargo fmt --all`;
    - any change to `desktop/` or a workspace manifest: `cargo test --workspace`,
      `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo fmt --all`, and
      `cargo clippy -p taskcascade-desktop --release --all-targets -- -D warnings`.
      Build `frontend/dist` first with `npm run build` from `frontend/`;
      `build.rs` panics in release without it. The desktop crate links WebView2
      and builds slowly, which is why the per-package form stays the backend default.
  - Frontend, from `frontend/`: `npm run check` and `npm run fmt`, plus
    `npm test` once Phase 7 adds it. Phase 10 records `npm test` in
    `Agents.md`, as Lars decided.
- **Architecture (`Agents.md`).**
  - Pure derivation lives in pure functions with unit tests: navigation
    classification, creation policy, identity selection, asset-policy checks,
    and the close and save state machines.
  - Code that must touch the filesystem (location identity) is tested against
    temporary directories.
  - HTTP and SQL stay in the backend `app` layer. Tauri, WebView2, path and
    process concerns stay in `desktop/`.
  - The React flow stays input → local draft (held by the save coordinator)
    → API mutation → refreshed state → render. API calls stay in
    `frontend/src/api.ts`.
  - The frontend's only contracts with the shell are the global
    `window.taskcascadeBeforeClose` and the two `/desktop/close-*` routes
    (Phase 7).
  - Markdown stays content: raw HTML rendering is never enabled.
  - Colours stay CSS tokens. The one native colour, the window background,
    mirrors `--canvas`.
- **Defaults exercise the new paths.** Single instance, link interception, the
  save coordinator, the close handshake, the shutdown deadline, the CSP and the
  WebView2 release settings are always on. The only opt-in is creating a
  missing database (`TASKCASCADE_CREATE_DATABASE=1`), and that opt-in is the
  safety rule itself. The development launcher sets it, so every development
  run exercises database creation.
- **Commits.** Each phase ends with exactly one commit, made after checking
  that `git status` shows only that phase's files. The headline is
  `Plan.DesktopWindow Phase N: <summary>`, and the message ends with the trailer
  line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`:

  ```powershell
  git add -A
  git commit -m "Plan.DesktopWindow Phase N: <summary>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
  ```

  This plan document is committed on its own before Phase 1, with the
  headline `Plan.DesktopWindow: add plan`.
- **Human verification.** Steps marked **Lars (required)** are by-eye verdicts
  or real-data operations that need Lars directly; they are not delegable.
  Steps marked **(recommended)** can be done by whoever implements the phase,
  on scratch or synthetic data.

## Decisions this plan settles

These are choices the brief left open, with the rationale for each. Durable
consequences reach the decision log in the final phase.

1. **The migration runs first, and a fixed snapshot bridges the interim.**
   - The data moves in Phase 1, before any new code exists that could write
     `.local/data` or create a database at the new location.
   - Until the installed copy exists (Phase 8), Lars runs the today's-browser
     app from a snapshot. The snapshot is created **once**, by an explicit
     `start.ps1 -CreateRealDataSnapshot`, from the verified, clean Phase 1
     commit. It holds the backend executable, `frontend/dist` and a manifest
     recording the git revision and the SHA-256 of every file.
   - `start.ps1 -RealData` only verifies that manifest and launches the
     snapshot. It never builds, copies or replaces anything.
   - Replacing the snapshot is a separate, explicit operation
     (`-CreateRealDataSnapshot -Force`, by Lars).
   - Agent rebuilds therefore can neither lock the daily app nor change it
     between restarts.
   - Both switches are removed in Phase 9.
2. **Scratch data lives in `.local/dev-data/`.** The old `.local/data/` is never
   reused. Only `start.ps1`'s default development mode refuses to start while
   `.local/data/taskcascade.sqlite` or any of its sidecars still exists, and
   points to the migration script. No parameter redirects or disables this
   guard. `-CreateRealDataSnapshot` skips it because snapshot creation touches
   no task data; `-RealData` skips it but still refuses before migration because
   `<RealDataRoot>\taskcascade.sqlite` does not exist yet.
3. **One resolved location decides identity and creation; the build profile
   decides defaults and assets.**
   - Release builds default to the real location and debug builds to
     `.local/dev-data/`. Both honour an absolute `TASKCASCADE_DATA_DIR`.
   - The requested directory is compared with the real directory by filesystem
     identity, not by spelling: file IDs when both exist, otherwise the
     canonical nearest existing ancestor plus a normalised remainder.
   - The result `is_real` drives the identity:
     - real → `se.pensjo.taskcascade`, titled "TaskCascade";
     - otherwise → `se.pensjo.taskcascade.dev`, titled "TaskCascade (dev)".
   - The same `is_real` drives the creation policy.
   - Ambiguous aliases are rejected at startup: a directory nested inside, or
     containing, the real directory, or a database file that is the real
     database under another name.
4. **Creation rule.** A missing database is created only if
   `TASKCASCADE_CREATE_DATABASE=1`, or if the build is a debug build and the
   location is not the real one. The real location is never created
   implicitly.
5. **The frontend is served by the desktop crate through `rust-embed`.**
   Debug builds read `frontend/dist` from disk at runtime, using the
   compile-time absolute path, so F5 in a development window picks up an
   `npm run build`. Release builds embed. The backend router becomes API-only,
   and `tower-http` leaves the backend in Phase 9.
6. **The database opens inside Tauri's `setup` hook.** It opens after the
   single-instance plugin has run, so a second launch hands off and exits
   before touching SQLite. The URI-scheme handler reads the router from a
   `OnceLock` that `setup` fills before it creates the window.
7. **The save coordinator owns drafts.**
   - Pending drafts, debounce timers and save state live in a coordinator
     above the keyed editor, keyed by task id. The editor renders the
     coordinator's draft, or the stored task when no draft exists. There is no
     props-to-draft copying effect.
     The mounted editor keeps showing the user's last typed fields when they equal the stored values after the server's normalisation.
   - Saves are serialized per task: at most one request per task is in flight,
     and newer edits coalesce into the next request.
   - A draft is dropped only when the revision the server confirmed is the
     newest one, so a late response can never replace a newer draft.
   - A failed save keeps its draft, shows the server's message and offers
     Retry. Completing, deleting and closing proceed only after a confirmed
     save of the task(s) concerned.
8. **The close handshake distinguishes availability from failure.**
   1. On the first close request the shell prevents the close and evaluates a
      fixed script.
   2. The script first posts `/desktop/close-ack` (the page is alive), then
      calls `window.taskcascadeBeforeClose()`.
   3. The page synchronously enters a closing state that freezes edits and
      mutations. It confirms every draft is saved, bounded at 10 s, and posts
      `/desktop/close-ready?outcome=saved`. On failure it posts `failed`,
      unfreezes, and shows a dialog: **Retry and close** / **Close and discard
      unsaved changes** / **Keep editing**.
   4. Timers behave differently by phase:
      - Only the 3-second acknowledgement deadline closes automatically, and
        it means that no live page answered.
      - Once the page has acknowledged, an explicit `failed` keeps the window
        open and no timer overrides it.
      - A 20-second backstop, and a hook that throws, each lead to a native
        "Close anyway?" question, never to a silent close.
   This uses no Tauri JavaScript API, consistent with decision 1 of the brief.
9. **One bounded shutdown.** Once closing is decided, a single 5-second
   deadline covers four stages:
   - stopping admission of new `/api` requests (they get 503);
   - draining in-flight requests, with up to 2 s;
   - closing the pool, with the remaining budget less 0.5 s;
   - runtime teardown (`shutdown_timeout`), with whatever is left.

   A stage that exhausts its budget is logged and skipped. The log
   distinguishes "edits confirmed saved; database closed cleanly" from
   "deadline exhausted at <stage>". A confirmed save means a committed
   transaction, so an exhausted pool close never loses a confirmed edit.
10. **Migration verification checks content, not just bytes.**
    - Besides SHA-256 identity of every moved file (TTTB precedent), the
      script runs the backend example `database_summary` on temporary copies
      of the source and target sets. The summary reports integrity, schema
      version, counts per task state and the newest modification time.
      SQLite therefore never opens the real files during the move.
    - A hot rollback journal (`-journal`) is refused, not moved. It means an
      interrupted transaction, and the script tells Lars to start and close
      the old app once so SQLite recovers it.
11. **No in-window probe.** TTTB's `--probe-webview` harness proved upload sizes
    and error envelopes that TaskCascade does not have. Bridge parity tests
    against the bare router cover TaskCascade's cases, together with the
    by-eye walkthroughs below: an error toast carrying the server's JSON
    message, the 204 preference save, search, and drag-and-drop.
12. **A small best-effort runtime log.**
    - Each run appends lifecycle lines to
      `<data dir>\logs\taskcascade-desktop.log` and echoes them to stderr. The
      lines cover the resolved database path and identity, the startup eval
      self-test, the close outcome, the shutdown report, failures and panics.
    - The file rotates once at 1 MB.
    - Release builds have no console, and the brief requires the resolved
      database path to be observable.
    - A failure to log never blocks startup.
13. **Release context menu (Claude's call, review round).** In release builds
    WebView2's default context menu appears only on editable text and text
    selections, giving cut, copy, paste and spell-check. It is suppressed
    everywhere else, so there is no Back, Reload, Save as or Print.

---

### Phase 1: Move the real tasks out of the repository

The real tasks are in the development scratch file
`.local/data/taskcascade.sqlite`. The current code does not set a journal mode,
and sqlx 0.8 leaves the file's existing mode unchanged. Sidecar files (`-wal`,
`-shm`, or a hot `-journal`) therefore exist only if WAL was configured at
some point or a transaction was interrupted, for example by `start.ps1`
force-killing the backend. The migration must handle both cases correctly and
must never assume which one applies.

This phase moves the data to `%LOCALAPPDATA%\TaskCascade\taskcascade.sqlite`
while the backend is still today's browser app, and sets up the fixed interim
snapshot. No desktop code exists yet.

**Files:**

- Modify:
  - `backend/src/app.rs` (`summarize_database`);
  - `backend/Cargo.toml` (`tempfile` dev-dependency);
  - `scripts/start.ps1` (scratch location, legacy guard,
    `-CreateRealDataSnapshot`, `-RealData`, rehearsal parameters, environment
    restore).
- Create:
  - `backend/examples/database_summary.rs` and
    `backend/examples/migration_fixture.rs`;
  - `backend/tests/database_summary.rs`;
  - `scripts/migrate-data.ps1`;
  - `scripts/tests/rehearse-migrate-data.ps1` and
    `scripts/tests/rehearse-real-data-snapshot.ps1`.

**Interfaces produced:**

```rust
// backend/src/app.rs — SQL stays in the app layer
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseSummary {
    pub integrity: String,                 // PRAGMA integrity_check, "ok" when healthy
    pub schema_version: Option<i64>,       // MAX(version) FROM _sqlx_migrations
    pub projects: i64,
    pub active_tasks: i64,                 // completed_at IS NULL AND deleted_at IS NULL
    pub archived_tasks: i64,               // completed_at IS NOT NULL AND deleted_at IS NULL
    pub deleted_tasks: i64,                // deleted_at IS NOT NULL
    pub preferences: i64,
    pub newest_modified_at: Option<String>,// MAX(modified_at) FROM tasks
}
pub enum SummaryError { Missing(PathBuf), Database(sqlx::Error) }
/// Opens an existing file without running migrations or seeding; never creates one.
pub async fn summarize_database(path: &Path) -> Result<DatabaseSummary, SummaryError>;
```

`database_summary <path>` prints the summary as one JSON line on stdout and
exits 1 with a message on stderr for any error.

`migration_fixture <empty dir> plain|wal` builds a synthetic database there
and writes `expected.json`, the summary the complete fixture must produce:

- **`plain`** creates a database in the default rollback-journal mode with
  known projects, active, archived and deleted tasks, and preferences, then
  closes cleanly. Only the main file remains.
- **`wal`** creates the same content, then opens a raw connection with
  `journal_mode = WAL` and `wal_autocheckpoint = 0`. It inserts a sentinel
  task titled `WAL-SENTINEL` (newer `modified_at`, one more active task),
  writes `expected.json` including the sentinel, and ends the process with
  `std::process::exit(0)` while the connection is still open. That leaves
  the sentinel committed only in `-wal`, absent from the main file.

- [ ] **Step 1: Write the failing tests**

`backend/tests/database_summary.rs` (temp directories only):

- **`summary_counts_tasks_by_state`.** Create a file database with
  `AppState::connect("sqlite://<tmp>/t.sqlite")`. Through `router`, using the
  `send()` helper pattern from `backend/tests/trash.rs`, create four tasks,
  complete one and delete one. Expect:
  - `integrity == "ok"`;
  - `projects == 1`;
  - `active_tasks == 2`, `archived_tasks == 1`, `deleted_tasks == 1`;
  - the highest migration number as `schema_version`;
  - a non-null `newest_modified_at`.
- **`summary_of_a_missing_file_is_an_error_and_creates_nothing`.** Returns
  `Missing`, and the directory stays empty.
- **`summary_reads_committed_wal_content`.**
  - Call the shared fixture builder `test_support::build_fixture(dir, Wal)`.
    It leaves its WAL connection deliberately unclosed
    (`std::mem::forget`), so nothing checkpoints.
  - Copy the main file together with its `-wal` and `-shm` into a second temp
    directory. Summarizing that copy equals the returned expected summary.
  - Summarizing a copy of the main file alone does **not** equal it: it
    reports one active task fewer. This proves the fixture really holds
    WAL-only data.

- [ ] **Step 2: Run tests to verify they fail**

Run (from `backend/`): `cargo test`
Expected: FAIL. `summarize_database` and the fixture logic do not exist.

- [ ] **Step 3: Implement `summarize_database` and both examples**

`summarize_database`:

- Checks `path.is_file()` first and returns `Missing` otherwise.
- Connects with `SqliteConnectOptions::new().filename(path).create_if_missing(false)`
  and a single-connection pool, without setting a journal mode.
- Runs the queries above, then calls `pool.close().await`.
- Runs no `sqlx::migrate!` and no seeding.

The examples are thin `#[tokio::main]` wrappers. The fixture-building logic
lives in `#[doc(hidden)] pub mod test_support` in the backend library:
`build_fixture(dir, FixtureKind) -> DatabaseSummary` returns the expected
summary. The `migration_fixture` example writes that summary as
`expected.json` and then ends with `std::process::exit(0)`, so the forgotten
WAL connection is never closed or checkpointed.

- [ ] **Step 4: Write `scripts/migrate-data.ps1`**

Adapt TTTB's `scripts/migrate-ledger.ps1` (commit 42f8bf4).

**Parameters.**

- `-SourcePath` (default `<repo>\.local\data\taskcascade.sqlite`).
- `-TargetRoot` (default `$env:LOCALAPPDATA\TaskCascade`; throw if
  `LOCALAPPDATA` is unset).
- `-SimulateFailureAfterMoves <n>`, a testing-only hook. The script refuses it
  unless both `-SourcePath` and `-TargetRoot` are given explicitly.

The destination file name stays `taskcascade.sqlite`. A comment names
`desktop/src/data_location.rs` (Phase 4) as the other definition of the
default.

**Steps.**

1. **Refuse unsafe states.** Refuse if any of the following holds:
   - a `taskcascade-backend` or `taskcascade-desktop` process is running;
   - the source is missing or not a regular file;
   - a `-journal` sidecar exists ("hot journal: start and close the old app
     once so SQLite recovers it, then rerun");
   - any source file (main, `-wal`, `-shm`) cannot be opened with
     `FileShare.None`;
   - `taskcascade.sqlite` or any of its `-wal`, `-shm` or `-journal` siblings
     already exists in the target root.
2. **Collect the source set.** Take the main file plus whichever of `-wal` and
   `-shm` exist, and record the size and SHA-256 of each.
3. **Pre-move content check.** Copy the set into a fresh
   `$env:TEMP\taskcascade-migrate-<stamp>\source\` and run
   `cargo run --quiet --manifest-path <repo>\backend\Cargo.toml --example database_summary -- <copy>`.
   Refuse unless `integrity == "ok"`, then print the summary.
4. **Safety copy.** Make a non-overwriting, hash-verified copy of the source
   set in `<TargetRoot>\pre-move-backup\`, using TTTB's reuse-if-identical and
   refuse-if-different rules.
5. **Move.** Create `<TargetRoot>` if needed, then move the sidecars first and
   the main file last.
   - With `-SimulateFailureAfterMoves n`, throw after the n-th move.
   - On any failure, roll back already-moved files. If rollback is incomplete,
     print TTTB's split-state text adapted: recover from `pre-move-backup` and
     do not start TaskCascade.
6. **Verify.**
   - Every moved file has the same SHA-256 and size as recorded in step 2.
   - Copy the moved set into `$env:TEMP\…\target\` and rerun
     `database_summary`. The whole summary must equal the pre-move summary.
   - Delete both temp copies.
7. **Report.**
   - The moved files, the summary and the safety-copy location.
   - Only when run with default paths: for each of
     `%USERPROFILE%\Documents\TaskCascade\taskcascade.sqlite` and
     `[Environment]::GetFolderPath('MyDocuments')\TaskCascade\taskcascade.sqlite`
     that exists, its path, size and last-write time, followed by "left
     untouched".
   - That `instances\` was not touched.
   - Next step: `.\scripts\start.ps1 -CreateRealDataSnapshot`.

- [ ] **Step 5: Update `scripts/start.ps1` (still the browser launcher)**

- **Scratch location.** `$dataDirectory` becomes the absolute
  `<repo>\.local\dev-data`.
- **Legacy guard.** In default development mode only, before any build, throw if
  `.local\data\taskcascade.sqlite`, `-wal`, `-shm` or `-journal` exists:
  "Your real tasks are still in .local\data. Run scripts\migrate-data.ps1
  first." No parameter redirects or disables the guard. `-CreateRealDataSnapshot`
  skips it because it touches no task data; `-RealData` skips it but still
  refuses before migration because `<RealDataRoot>\taskcascade.sqlite` does
  not exist yet.
- **Rehearsal parameters.**
  - `-RealDataRoot` (default `$env:LOCALAPPDATA\TaskCascade`) and
    `-SnapshotDir` (default `<repo>\.local\real-run`). They exist only so
    rehearsals can use synthetic roots.
  - The banner always prints the actual root.
- **`-CreateRealDataSnapshot` (interim; explicit, one-off).**
  - **Refuse when:**
    - the working tree is dirty (`git status --porcelain` is non-empty);
    - `-SnapshotDir` already holds a snapshot and `-Force` was not given;
    - a snapshot executable is running.
  - **Build:** `npm install`, `npm run build`, and the backend debug build.
  - **Copy:** `taskcascade-backend.exe` into `<SnapshotDir>\backend\` and
    `frontend\dist` into `<SnapshotDir>\frontend\dist\`.
  - **Manifest:** write `<SnapshotDir>\snapshot.json` with the git revision,
    the UTC creation time, and the relative path and SHA-256 of every
    snapshot file.
- **`-RealData` (interim, removed in Phase 9).** It never runs npm or cargo
  and never copies.
  1. Require `<RealDataRoot>\taskcascade.sqlite` to exist; the script never
     creates it.
  2. Require `snapshot.json`, and verify every listed hash. Also verify that no
     unlisted file exists under `<SnapshotDir>\frontend\dist`. On any mismatch,
     refuse: "the snapshot changed since it was created — recreate it
     explicitly with -CreateRealDataSnapshot -Force".
  3. Refuse if a snapshot executable is already running.
  4. Start `<SnapshotDir>\backend\taskcascade-backend.exe` with working
     directory `<SnapshotDir>\backend`, so its CWD-relative
     `ServeDir("../frontend/dist")` resolves to the snapshot. Set
     `TASKCASCADE_DATA_DIR=<RealDataRoot>` and a free port.
  5. Do not start Vite. Print and open `http://127.0.0.1:<port>/`.
  6. Print the banner: "REAL DATA — <RealDataRoot> — snapshot <revision>".
- **Environment.** Capture the previous `TASKCASCADE_DATA_DIR` and
  `TASKCASCADE_PORT` values, including "unset", and restore them in `finally`.
- **Combinations.** `-RealData` and `-CreateRealDataSnapshot` are mutually
  exclusive with each other and with the default development mode. Without
  either switch, behaviour is as today (Vite plus backend) on the scratch
  location.

- [ ] **Step 6: Write the rehearsal scripts (synthetic fixtures only)**

Both scripts take `-Root <temp dir>`. They refuse unless that root is absent
or empty, and refuse if it resolves inside `%LOCALAPPDATA%\TaskCascade` or
`<repo>\.local\data`. They create everything under the root and delete the
root at the end.

`scripts/tests/rehearse-migrate-data.ps1` asserts each of these and prints
PASS or FAIL per case:

1. **WAL fixture migrates completely.** Generate the `wal` fixture with
   `migration_fixture` and run
   `migrate-data.ps1 -SourcePath <root>\wal\src\taskcascade.sqlite -TargetRoot <root>\wal\dst`.
   The target summary equals `expected.json`, including the sentinel's
   `newest_modified_at` and active count. The safety copy and the hashes are
   verified.
2. **The fixture really needs its WAL.** A copy of the `wal` fixture's main
   file alone summarizes differently from `expected.json`.
3. **Plain fixture migrates.** The `plain` fixture migrates, and its target
   summary equals `expected.json`.
4. **Existing target is refused.** Re-running case 3's command refuses, and
   nothing changes.
5. **Locked source is refused.** A fresh `plain` source held open with
   `[IO.File]::Open(<path>,'Open','ReadWrite','None')` in a background job is
   refused.
6. **Hot journal is refused.** A fresh `plain` source with an empty
   `taskcascade.sqlite-journal` placed beside it is refused.
7. **Forced mid-move failure recovers.** A fresh `wal` source with
   `-SimulateFailureAfterMoves 1` fails; the source set is intact (hashes
   equal the originals) and the target holds no database files.
8. **Real-data paths untouched.** Before case 1 and after case 7, record the
   existence, size and last-write time of
   `%LOCALAPPDATA%\TaskCascade\taskcascade.sqlite*` and
   `.local\data\taskcascade.sqlite*` without opening them; the two records
   must be identical. The script never passes those paths to anything.

`scripts/tests/rehearse-real-data-snapshot.ps1`:

1. Generate a `plain` fixture as `<root>\data\taskcascade.sqlite`.
2. Run
   `start.ps1 -CreateRealDataSnapshot -SnapshotDir <root>\snap -Force`. `-Force`
   is allowed only for a fresh root. A dirty tree is allowed only through
   `-AllowDirtyForRehearsal`, which the script accepts only together with a
   non-default `-SnapshotDir`.
3. Record `snapshot.json` and the snapshot files' hashes and last-write times.
4. Change repository artefacts: `npm run build`, a backend `cargo build`, and
   one extra file dropped into `frontend\dist`.
5. Launch
   `start.ps1 -RealData -RealDataRoot <root>\data -SnapshotDir <root>\snap`
   as a background job, with `PATH` stripped of `cargo` and `npm`.
6. Poll `/api/bootstrap` until it answers with the fixture's tasks, then stop
   the process tree.
7. Assert that the snapshot hashes, last-write times and manifest are
   unchanged, and that the extra dist file is absent from the snapshot. This
   proves no build or copy happened.
8. Tamper with one snapshot file. `-RealData` must refuse.

- [ ] **Step 7: Run tests, the gate and both rehearsals**

Run (from `backend/`): `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`.
Then run both rehearsal scripts with a fresh `<scratchpad>\rehearsal-<n>` root.
Expected: every check passes, including the 16 existing tests, and every
rehearsal case prints PASS.

- [ ] **Step 8: Commit**

Headline: `Plan.DesktopWindow Phase 1: move the real tasks to app data`.

- [ ] **Step 9: Lars (required): migrate, snapshot, switch to the interim launcher**

1. Stop `start.ps1` and close the browser tab.
2. Run `.\scripts\migrate-data.ps1` with no arguments. Compare the printed
   summary with what the app showed.
3. With the tree clean at the Phase 1 commit, run
   `.\scripts\start.ps1 -CreateRealDataSnapshot`.
4. Run `.\scripts\start.ps1 -RealData`. Every task, project, the archive and
   the trash are present. Edit one task, then stop the script.
5. `.\scripts\start.ps1` with no switch opens an empty scratch stack that
   contains only "Personal".

**Phase 2 does not start until this step is done.**

---

### Phase 2: Cargo workspace

This phase changes no behaviour. It prepares a shared `target/` and lockfile
for the desktop crate. The interim snapshot is not touched.

**Files:**

- Create: `Cargo.toml` (root).
- Move: `backend/Cargo.lock` → `Cargo.lock`, using `git mv` so the resolved
  versions stay pinned.
- Modify:
  - `backend/Cargo.toml`: `version.workspace = true` and
    `edition.workspace = true`.
  - `scripts/start.ps1`: development builds and `-CreateRealDataSnapshot`
    build from the root, and the executable moves to
    `target\debug\taskcascade-backend.exe`.
  - `.gitignore`: drop the redundant `backend/target/` line, since the
    generic `target` entry covers the root.

```toml
[workspace]
resolver = "2"
members = ["backend"]

[workspace.package]
version = "0.1.0"
edition = "2021"
```

- [ ] **Step 1: Create the workspace and move the lockfile**

`sqlx::migrate!("./migrations")` resolves against the backend manifest
directory and needs no change. `migrate-data.ps1` and the rehearsals keep
working, because `--manifest-path backend\Cargo.toml` is valid inside a
workspace.

- [ ] **Step 2: Point `start.ps1` at the workspace**

Development builds and `-CreateRealDataSnapshot` now run
`cargo build -p taskcascade-backend` from the repository root and use the new
executable path. `-RealData` is unchanged: it launches the existing snapshot,
and its manifest still names the Phase 1 revision.

- [ ] **Step 3: Remove the stale `backend/target/`**

Remove it only after confirming no backend process runs from it. It is a
rebuildable artefact.

- [ ] **Step 4: Run the gate and the snapshot rehearsal**

Run (from the repository root):
`cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`.
Then run `scripts\tests\rehearse-real-data-snapshot.ps1` with a fresh root.
Expected: PASS. `target\` sits at the root, and `git status` shows
`Cargo.lock` as a rename.

- [ ] **Step 5: Human verification**

- **(recommended)** `.\scripts\start.ps1` runs the browser app on scratch
  data.
- **Lars:** `.\scripts\start.ps1 -RealData` starts the same Phase 1 snapshot
  (the banner shows the Phase 1 revision) and shows his tasks.

- [ ] **Step 6: Commit**

Headline: `Plan.DesktopWindow Phase 2: make the repository a Cargo workspace`.

---

### Phase 3: Walking skeleton — the app in a native window, no port

This is the smallest end-to-end slice: a debug-only development window loads
the full UI and API through the custom scheme, on scratch data. The browser
path keeps working beside it until Phase 9.

**Files:**

- Modify:
  - `Cargo.toml`: members `["backend", "desktop"]`.
  - `backend/src/app.rs`: `router` becomes API-only. Drop
    `.fallback_service(ServeDir…)` and `.layer(CorsLayer::permissive())`.
  - `backend/src/main.rs`: the browser shell now composes
    `router(state).fallback_service(ServeDir::new("../frontend/dist"))`
    itself, without CORS.
  - `frontend/index.html`: the favicon link.
  - `frontend/vite.config.ts`: `build.assetsInlineLimit: 0`.
  - `scripts/start.ps1`: the `-Desktop` switch.
  - `.gitignore`: `desktop/gen/`.
- Create:
  - `frontend/public/favicon.svg` (the current inline SVG, decoded);
  - `desktop/Cargo.toml`, `desktop/build.rs` and `desktop/tauri.conf.json`;
  - `desktop/icons/icon.ico`, plus `desktop/icons/README.md` naming the
    regeneration command and its master, `frontend/public/favicon.svg`;
  - `desktop/src/{main.rs, lib.rs, launch.rs, bridge.rs, assets.rs, asset_policy.rs, content_security_policy.rs}`.

**Interfaces produced:**

```rust
// bridge.rs
pub const SCHEME: &str = "taskcascade";
pub fn launch_url() -> tauri::WebviewUrl;                       // taskcascade://localhost/
pub const PAGE_ORIGIN: &str = "http://taskcascade.localhost";   // as WebView2 sees it
/// Feeds one webview request into the router: path-and-query only, body passed
/// through, response body collected, headers copied, CSP header inserted.
pub async fn serve_request(router: Router, request: http::Request<Vec<u8>>) -> http::Response<Vec<u8>>;

// assets.rs
pub struct Asset { pub bytes: std::borrow::Cow<'static, [u8]>, pub content_type: String }
pub trait AssetSource: Clone + Send + Sync + 'static { fn get(&self, path: &str) -> Option<Asset>; }
#[derive(rust_embed::RustEmbed, Clone)] #[folder = "../frontend/dist"] pub struct FrontendDist;
impl AssetSource for FrontendDist { /* … */ }
/// GET "/" → index.html; GET "/<file>" → that file; anything else → 404. No SPA fallback.
pub fn router<S: AssetSource>(source: S) -> Router;

// asset_policy.rs — shared with build.rs via #[path]
pub fn violations(index_html: &str) -> Vec<String>;  // inline <script> without src, any "data:" URI

// content_security_policy.rs
pub const VALUE: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
  font-src 'self'; img-src 'self' https:; connect-src 'self'; object-src 'none'; base-uri 'self'; \
  form-action 'none'; frame-ancestors 'none'";

// launch.rs
pub fn compose<S: AssetSource>(state: AppState, assets: S) -> Router; // app::router(state).fallback_service(assets::router(assets))
pub fn run() -> Result<(), Box<dyn std::error::Error>>;
```

`desktop/Cargo.toml`:

- Package `taskcascade-desktop`, with `version.workspace` and
  `edition.workspace`.
- Dependencies:
  - `axum = "0.8"`;
  - `http = "1"`;
  - `tauri = "2"`;
  - `tokio`, with `rt-multi-thread`, `time` and `sync`;
  - `tower`, with `util`;
  - `rust-embed`, with the `mime-guess` feature;
  - `taskcascade-backend = { path = "../backend" }`.
- Build dependency `tauri-build = "2"`, dev dependency `tempfile`.
- `[[bin]]` name `taskcascade-desktop`. The name `taskcascade` stays reserved
  for a possible console CLI.

`tauri.conf.json`:

- `productName` "TaskCascade" and `identifier` `se.pensjo.taskcascade`;
- `build: {}`;
- `app.withGlobalTauri: false` and `app.windows: []`;
- `app.security.csp: null`, because the bridge emits the policy and Tauri's
  setting does not cover custom-protocol responses;
- `bundle.active: false`.

There is no `capabilities/` directory.

- [ ] **Step 1: Write the failing tests**

Desktop unit tests (`cargo test -p taskcascade-desktop`):

- **`bridge`, parity with the bare router.** Build a router from
  `AppState::connect("sqlite::memory:")` composed with an in-memory
  `AssetSource` fixture. Status, body and `content-type` must match a direct
  `oneshot` for each of:
  - `GET /api/bootstrap`;
  - `POST /api/tasks` with a JSON body;
  - a 400 validation error (empty title), whose JSON `message` must survive;
  - `PUT /api/preferences/projectFilter`, whose 204 has an empty body;
  - an unknown `/api/nope`, which returns 404.
- **`bridge`, URI forms.** `taskcascade://localhost/api/health?x=1`,
  `http://taskcascade.localhost/api/health?x=1` and `/api/health?x=1` all reach
  the same handler.
- **`bridge`, CSP everywhere.** `/`, `/assets/app.js` and `/api/health` all
  carry `Content-Security-Policy == VALUE`.
- **`assets`.**
  - `/` serves `text/html`, and `/assets/x.js` serves a JavaScript content
    type.
  - A missing file returns 404.
  - `/..%2F..%2FCargo.toml` and `/../Cargo.toml` return 404.
  - Every non-GET request returns 405 or 404.
- **`asset_policy`.**
  - It detects `<script>inline()</script>` and `href="data:…"`.
  - It accepts `<script type="module" crossorigin src="/assets/index-abc.js"></script>`.

- [ ] **Step 2: Run tests to verify they fail**

Run (from the repository root): `cargo test --workspace`
Expected: FAIL. The desktop crate has no implementation yet.

- [ ] **Step 3: Make the backend router API-only**

Move `ServeDir` into `backend/src/main.rs` and delete the CORS layer
altogether. Every remaining browser path is same-origin: either the Vite proxy
or the backend serving `dist` itself. The 19 backend tests stay untouched and
must pass.

- [ ] **Step 4: Frontend prerequisites for the CSP**

- **Favicon.** Decode the data-URI SVG into `frontend/public/favicon.svg` and
  link `/favicon.svg`.
- **No inlined assets.** Set `build.assetsInlineLimit: 0`. `font-src 'self'`
  would block an inlined font, and `@fontsource` ships small subsets.
- **Check.** Run `npm run build` and confirm `dist/index.html` has no inline
  script and no `data:`.

- [ ] **Step 5: Icon**

Generate `desktop/icons/icon.ico` (16–256 px) from `frontend/public/favicon.svg`
with
`npx --yes @tauri-apps/cli@2 icon frontend/public/favicon.svg -o <scratchpad>\icons`,
and copy only `icon.ico` into the repository. If the CLI rejects SVG, render a
1024 px PNG first (for example with headless Edge `--screenshot`) and feed
that to the CLI instead. Record the command in `desktop/icons/README.md`.

- [ ] **Step 6: Implement the crate**

- **`build.rs`:**
  - calls `tauri_build::build()` and emits
    `cargo:rerun-if-changed=../frontend/dist`;
  - in release builds (`PROFILE == "release"`), panics with a clear message
    if `../frontend/dist/index.html` is missing or `asset_policy::violations`
    is non-empty;
  - in debug builds, only warns.
- **`launch::run`:**
  1. Build a multi-thread tokio runtime manually, with no `#[tokio::main]`,
     because Tauri owns the main thread.
  2. Resolve the data directory. For this phase only: the absolute
     `TASKCASCADE_DATA_DIR` if set, otherwise the compile-time anchor
     `env!("CARGO_MANIFEST_DIR")/../.local/dev-data`. Create it and open it
     with `AppState::connect`. Phase 4 replaces this.
  3. In debug builds, fail with "frontend not built — run npm run build in
     frontend/" if `FrontendDist::get("index.html")` is `None`.
  4. Compose the router. `register_asynchronous_uri_scheme_protocol(SCHEME, …)`
     spawns `serve_request` on the runtime.
  5. In `setup`, build window `"main"` with
     `WebviewWindowBuilder::new(app, "main", launch_url())`:
     - title "TaskCascade (dev)";
     - inner size 1280×800, minimum 900×560;
     - background `#0a0b0d` (the `--canvas` token);
     - **`.disable_drag_drop_handler()`**, so HTML5 drag-and-drop reaches the
       page on Windows.
  6. Call `run_return`, then drop the state. The bounded shutdown arrives in
     Phase 4.
- **`main.rs`** calls `launch::run()`.

- [ ] **Step 7: `start.ps1 -Desktop`**

1. Run `npm run build`, then `cargo build -p taskcascade-desktop` from the
   root.
2. Set `TASKCASCADE_DATA_DIR` to the absolute `.local\dev-data`.
3. Run `target\debug\taskcascade-desktop.exe` with
   `Start-Process -Wait -NoNewWindow`, and restore the environment in
   `finally`.
4. Print: "Close the window to stop. Ctrl+C here hard-kills the window without
   saving a pending edit."

`-Desktop` cannot be combined with `-RealData` or `-CreateRealDataSnapshot`.

- [ ] **Step 8: Run the gates**

Run (from the repository root):
`cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`.
Run (from `frontend/`): `npm run check && npm run fmt`.
Expected: PASS.

- [ ] **Step 9: Human verification (recommended): the window works on scratch data**

Run `.\scripts\start.ps1 -Desktop`, then in the window:

1. Create a project and tasks, and edit a description.
2. Drag a task onto Archive, Deleted and back to All tasks.
3. Reorder by drag and by Ctrl+Up/Down.
4. Search.
5. Create a project whose name already exists. The server answers with a JSON
   `message`, and the toast must show it verbatim.
6. Toggle a project filter. That is the 204 preference save.

Fonts render, and the devtools console (debug builds only) shows no CSP
violations.
`Get-NetTCPConnection -State Listen -OwningProcess (Get-Process taskcascade-desktop).Id`
returns nothing. After closing the window, `.\scripts\start.ps1` (browser)
still works.

- [ ] **Step 10: Commit**

Headline: `Plan.DesktopWindow Phase 3: open TaskCascade in a native window over an in-process scheme`.

---

### Phase 4: Where the data is, refusing to guess, and a bounded shutdown

After this phase, a release build would open the real database. Agents still
run release builds only with a scratch override.

**Files:**

- Modify:
  - `backend/src/app.rs`: `OpenMode`, `OpenError`, `AppState::open`,
    `AppState::close`, and a `#[doc(hidden)] AppState::acquire_for_tests`.
  - `desktop/src/launch.rs` and `desktop/src/lib.rs`.
  - `desktop/Cargo.toml`: add `same-file = "1"`, and `windows-sys` with
    `Win32_Foundation` and `Win32_UI_WindowsAndMessaging` under
    `cfg(windows)`.
- Create:
  - `backend/tests/open_database.rs`;
  - `desktop/src/data_location.rs`, `desktop/src/startup_failure.rs`,
    `desktop/src/diagnostics.rs` and `desktop/src/lifecycle.rs`.

**Interfaces produced:**

```rust
// backend/src/app.rs
pub enum OpenMode { ExistingOnly, CreateIfMissing }
#[derive(Debug)] pub enum OpenError { Missing(PathBuf), Database(sqlx::Error) }
impl AppState {
    pub async fn open(database: &Path, mode: OpenMode) -> Result<Self, OpenError>; // migrates + seeds as connect does
    pub async fn close(&self);   // pool.close(): waits for checked-out connections; callers bound it
    #[doc(hidden)] pub async fn acquire_for_tests(&self) -> sqlx::pool::PoolConnection<sqlx::Sqlite>;
}

// desktop/src/data_location.rs
pub struct LocationInputs {
    pub data_dir_override: Option<std::ffi::OsString>, // TASKCASCADE_DATA_DIR
    pub create_opt_in: bool,                           // TASKCASCADE_CREATE_DATABASE == "1"
    pub local_app_data: Option<PathBuf>,               // %LOCALAPPDATA% (tests pass a temp dir)
    pub debug_build: bool,                             // cfg!(debug_assertions)
    pub dev_default: PathBuf,                          // CARGO_MANIFEST_DIR/../.local/dev-data
}
pub struct DataLocation { pub directory: PathBuf, pub database: PathBuf, pub is_real: bool, pub may_create: bool }
pub enum LocationError {
    RelativeOverride(PathBuf),
    NoLocalAppData,
    Unresolvable { path: PathBuf, error: std::io::Error },
    AmbiguousAlias { requested: PathBuf, real: PathBuf, reason: &'static str },
}
pub fn real_directory(local_app_data: &Path) -> PathBuf;                      // <LOCALAPPDATA>\TaskCascade
pub fn requested_directory(inputs: &LocationInputs) -> Result<PathBuf, LocationError>; // pure: override / profile default
pub struct ResolvedPath { pub canonical: PathBuf, pub exists: bool }
pub fn filesystem_identity(path: &Path) -> Result<ResolvedPath, LocationError>; // filesystem step
pub fn creation_policy(is_real: bool, debug_build: bool, opt_in: bool) -> bool; // pure
pub fn resolve(inputs: &LocationInputs) -> Result<DataLocation, LocationError>; // composes the above

// desktop/src/startup_failure.rs
pub fn failure_message(window_loop_started: bool, error: &str, database: &str, log: &str) -> String;
pub fn show(title: &str, message: &str);   // MessageBoxW on Windows, eprintln elsewhere

// desktop/src/lifecycle.rs
pub const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(5);
pub struct ShutdownReport { pub drained: bool, pub pool_closed: bool, pub elapsed: Duration, pub exhausted_at: Option<&'static str> }
pub struct Lifecycle { /* Mutex<Option<AppState>>, Option<Arc<InFlight>> from Phase 7 */ }
impl Lifecycle {
    /// Idempotent; the second call returns the first report.
    pub async fn shutdown(&self, deadline: Duration) -> ShutdownReport;
}
```

Resolution rules:

1. **Requested directory.** A set override must be absolute; otherwise
   `RelativeOverride`. With no override, a release build uses the real
   directory (`NoLocalAppData` if `LOCALAPPDATA` is unset; never a
   working-directory fallback), and a debug build uses `dev_default`.
2. **Filesystem identity.**
   - **Existing path:** `std::fs::canonicalize`, which resolves `.`/`..`,
     junctions, symbolic links, short names and case. An error here is
     `Unresolvable`, never a silent fallback.
   - **Missing path:** apply `std::path::absolute` (which on Windows
     normalises `.` and `..`), canonicalize the nearest existing ancestor, and
     append the remaining components.
   - The same is done for the real directory.
3. **Comparison.**
   - If both directories exist, `is_real` is `same_file::is_same_file` on the
     directories, which compares volume and file IDs and so catches every
     alias spelling.
   - Otherwise, `is_real` is equality of the canonical forms (case-insensitive
     on Windows).
4. **Rejected aliases (`AmbiguousAlias`).**
   - The requested directory lies strictly inside the real directory, or
     contains it.
   - The directories differ, but `<requested>\taskcascade.sqlite` and
     `<real>\taskcascade.sqlite` both exist and are the same file (a hard link
     or symbolic link).
5. **Creation policy.**
   `may_create = creation_policy(is_real, debug_build, create_opt_in)`, which
   is `opt_in || (debug_build && !is_real)`. Phase 5 derives the identity from
   this same `is_real`.
6. **Database path.** The database is always `<directory>\taskcascade.sqlite`,
   using the requested spelling for display and the canonical form for the
   log.

- [ ] **Step 1: Write the failing tests**

`backend/tests/open_database.rs` (temp directories):

- `existing_only_refuses_a_missing_file_and_creates_nothing`.
- `create_if_missing_creates_migrates_and_seeds_personal`.
- `existing_only_opens_and_migrates_an_existing_file`.
- `close_releases_the_file`: after `close()` the file can be renamed.

`data_location` tests use a temp directory as `local_app_data`, so the real
directory is `<tmp>\TaskCascade`. They never touch the actual real directory.

- **Defaults and policy:**
  - the release default is real and `!may_create`;
  - the release default with the opt-in can create;
  - the debug default is `dev_default`, `may_create` and `!is_real`;
  - an absolute override wins in both profiles;
  - a relative override is rejected;
  - missing `LOCALAPPDATA` in release returns `NoLocalAppData`;
  - `creation_policy` truth table.
- **Aliases of the real directory.** Each of these resolves to `is_real`,
  `!may_create` in debug, with the identical `DataLocation`, both when
  `<tmp>\TaskCascade` exists and when it does not:
  - `<tmp>\TaskCascade\..\TaskCascade`;
  - `<tmp>\.\TaskCascade`;
  - upper-cased `<TMP>\TASKCASCADE`;
  - `\\?\<tmp>\TaskCascade` (extended-length);
  - a trailing separator.
- **Junction** (`#[cfg(windows)]`, created with `cmd /C mklink /J <tmp>\alias <tmp>\TaskCascade`):
  `<tmp>\alias` is real.
- **Rejected:**
  - `<tmp>\TaskCascade\scratch` (nested);
  - `<tmp>` itself (containing the real directory);
  - a scratch directory whose `taskcascade.sqlite` is a hard link (`std::fs::hard_link`) to `<tmp>\TaskCascade\taskcascade.sqlite`.

  Each returns `AmbiguousAlias`.
- **Unrelated scratch:** `<tmp>\scratch` is not real and, in debug builds, may
  create.

`startup_failure::failure_message` names the error, the database path and the
log path, and distinguishes "could not start" from "stopped unexpectedly".

`lifecycle` tests (tokio, file database in a temp directory):

- **Normal close.** Create a task through the router, then shut down with
  `SHUTDOWN_DEADLINE`. The report has `pool_closed` and no `exhausted_at`. The
  file can be renamed, and reopening it shows the task.
- **Held connection.** Hold a connection from `acquire_for_tests()` and shut
  down with a 600 ms deadline, exercising the pool-close timeout after the
  500 ms runtime reserve. It returns in under about 700 ms with
  `pool_closed == false` and `exhausted_at == Some("pool close")`.
- **Budget exhausted before pool close starts.** With a 500 ms deadline, the
  500 ms runtime reserve leaves no pool-close budget. Shutdown skips `close()`
  and reports `pool_closed == false` and `exhausted_at == Some("pool close")`;
  a new connection can still be acquired.
- **Idempotent.** A second `shutdown` returns at once with the same report.

- [ ] **Step 2: Run tests to verify they fail**

Run (from the repository root): `cargo test --workspace`
Expected: FAIL.

- [ ] **Step 3: Implement the backend opening rules**

For `ExistingOnly`, `open` checks `database.is_file()` before sqlx connects.
Both modes then share `connect`'s migrate, seed and colour steps, factored into
one private function. `connect(url)` stays for in-memory tests.

- [ ] **Step 4: Implement location, diagnostics, failure reporting and the bounded shutdown**

- **Location.** `launch::run` reads the environment once into `LocationInputs`
  and calls `resolve`. Every `LocationError` becomes a startup failure whose
  dialog names the requested and real paths and the reason.
- **Diagnostics.** Open `<directory>\logs\taskcascade-desktop.log` best-effort,
  rotating once to `.1` at 1 MB. Write each line to stderr and to the file,
  with a timestamp and the pid. The first line records the build profile, the
  version, the requested and canonical database paths, `is_real` and
  `may_create`.
- **Database opening.** Open the database inside `setup`, blocking on the
  runtime, and store the composed router in a `OnceLock<Router>` that the
  scheme handler reads. The handler answers 503 if the lock is empty. If the
  database is missing and `!may_create`, the error reads:

  ```text
  No task database at <path>.
  TaskCascade does not create an empty database on its own. If your tasks were
  moved, put taskcascade.sqlite back. To start with an empty database, start
  TaskCascade once with TASKCASCADE_CREATE_DATABASE=1 (scripts\install.ps1 -InitDatabase).
  ```

- **Startup failures.** Tauri 2.12.5 runs setup inside `run_return` and panics
  on a setup `Err`, so capture setup errors in a shared slot and request exit.
  After `run_return`, `run` reports them as "could not start", logs them,
  shows `failure_message` via `MessageBoxW` and exits 1. Startup is marked
  successful only after setup succeeds.
  Install a panic hook that records the message, wrap the run in
  `catch_unwind`, and report a panic the same way (TTTB's `PanicReport`).
- **Bounded shutdown.** Both the `RunEvent::Exit` handler and the code after
  `run_return` call `Lifecycle::shutdown(SHUTDOWN_DEADLINE)` via
  `runtime.block_on`, sharing one deadline.
  - It takes the state.
  - Phase 7 adds the admission stop and the drain at this point.
  - `tokio::time::timeout(remaining - 500 ms, state.close())`.
  - It logs the report.
  - Afterwards, `run` calls `runtime.shutdown_timeout(remaining.max(250 ms))`
    instead of dropping the runtime, so runtime teardown is inside the same
    budget.

- [ ] **Step 5: Run the gate**

Run (from the repository root):
`cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

- [ ] **Step 6: Human verification (recommended; scratch only)**

1. Run `npm run build`, then `cargo build -p taskcascade-desktop --release`.
2. **Missing database refused.** Set `TASKCASCADE_DATA_DIR=<scratchpad>\empty`
   with no opt-in. A native dialog names `<scratchpad>\empty\taskcascade.sqlite`,
   no `.sqlite` file appears, and the log in `<scratchpad>\empty\logs\` holds
   the same lines.
3. **Opt-in creates.** With `TASKCASCADE_CREATE_DATABASE=1`, the window opens
   on an empty stack with "Personal".
4. **Relative override.** `TASKCASCADE_DATA_DIR=foo` gives a dialog explaining
   that it must be absolute.
5. **Debug default.** A debug run with no environment opens
   `.local\dev-data`, and the log names the canonical path.

Restore the environment afterwards.

- [ ] **Step 7: Commit**

Headline: `Plan.DesktopWindow Phase 4: resolve the task database by identity and shut down within a deadline`.

---

### Phase 5: Window identity and lifecycle

**Files:**

- Modify: `desktop/Cargo.toml` (`tauri-plugin-window-state = "2"`,
  `tauri-plugin-single-instance = "2"`), `desktop/src/main.rs`,
  `desktop/src/launch.rs`, `desktop/src/lib.rs`.
- Create: `desktop/src/identity.rs`.

**Interfaces produced:**

```rust
pub const REAL_IDENTIFIER: &str = "se.pensjo.taskcascade";
pub const DEV_IDENTIFIER: &str = "se.pensjo.taskcascade.dev";
pub struct Identity { pub identifier: &'static str, pub title: &'static str }
pub fn identity_for(location: &DataLocation) -> Identity;  // is_real → ("…taskcascade", "TaskCascade"); else (".dev", "TaskCascade (dev)")
```

- [ ] **Step 1: Write the failing tests**

`identity_for` unit tests:

- the real location;
- the debug default;
- a release build with a scratch override;
- every Phase 4 alias of the real directory, resolved through `resolve` with
  a temp `local_app_data`. Each gets the real identity, so instance scope and
  creation policy always agree.

- [ ] **Step 2: Run tests to verify they fail**

Run (from the repository root): `cargo test -p taskcascade-desktop`
Expected: FAIL.

- [ ] **Step 3: Apply the identity before Tauri builds**

Set the identifier before `Builder::build(context)`:

```rust
let mut context = tauri::generate_context!();
context.config_mut().identifier = identity.identifier.into();
```

The identifier scopes the WebView2 profile
(`%LOCALAPPDATA%\<identifier>\EBWebView`), the window-state file and the
single-instance lock. Log the identity.

- [ ] **Step 4: Single instance, window state, show after restore**

- **Single instance.** Register `tauri_plugin_single_instance::init(...)`
  **first** in the builder chain. Its callback unminimizes, shows and focuses
  window `"main"`. Because the database opens in `setup`, a second launch
  exits before touching SQLite.
- **Window state.** Register
  `tauri_plugin_window_state::Builder::default().with_state_flags(StateFlags::all() - StateFlags::VISIBLE)`.
- **Show after restore.** Build the window with `.visible(false)` and the
  identity title. Show it from `app.run_on_main_thread`, queued after
  `build()` (TTTB's FIFO note), so the restored placement applies before the
  first paint.

- [ ] **Step 5: No console in release**

Add `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` to
`main.rs`.

- [ ] **Step 6: Run the gate**

Run (from the repository root):
`cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: PASS.

- [ ] **Step 7: Human verification (recommended; scratch only)**

1. **Placement.** Run `.\scripts\start.ps1 -Desktop`, move and resize the
   window, close it and reopen it. The placement is restored, with no white
   flash.
2. **Single instance.** A second launch of
   `target\debug\taskcascade-desktop.exe` with the same environment focuses
   the first window. The log shows no second database open.
3. **Release on scratch.** A release build with a scratch
   `TASKCASCADE_DATA_DIR` and the opt-in has the title "TaskCascade (dev)" and
   no console. Started while a debug development window is open, it focuses
   that window.
4. **Icon.** The taskbar and Alt+Tab show the icon.

- [ ] **Step 8: Commit**

Headline: `Plan.DesktopWindow Phase 5: separate dev and real identities with one window per copy`.

---

### Phase 6: The window behaves like an app, not a browser

**Files:**

- Modify:
  - `desktop/Cargo.toml`: `tauri-plugin-opener = "2"` and `url = "2"`. Add
    `webview2-com` and `windows` only if Step 4 needs them, pinned to exactly
    the versions `wry` resolves in `Cargo.lock`.
  - `desktop/src/launch.rs` and `desktop/src/lib.rs`.
- Create: `desktop/src/navigation.rs` and `desktop/src/webview_settings.rs`.

**Interfaces produced:**

```rust
pub enum NavigationDecision { Allow, OpenExternally, Block }
/// Allow only the app's own root document (initial load and reload);
/// http/https/mailto leave for the system handler; everything else is blocked.
pub fn classify(url: &url::Url) -> NavigationDecision;
/// Release-build context menu policy (decision 13).
pub fn keep_default_context_menu(target_is_editable: bool, has_selection: bool) -> bool;
```

- [ ] **Step 1: Write the failing tests**

`navigation::classify`:

- **`Allow`:** `http://taskcascade.localhost/` and `taskcascade://localhost/`.
- **`Block`:**
  - `http://taskcascade.localhost/notes.md`;
  - `http://taskcascade.localhost/?x`;
  - `file:///C:/x`;
  - `javascript:alert(1)`;
  - `data:text/html,x`;
  - `ftp://h/x`;
  - `ms-settings:`;
  - `taskcascade://localhost/api/health`.
- **`OpenExternally`:** `https://example.com/a?b#c`, `http://example.com` and
  `mailto:a@b.c`.

`keep_default_context_menu`: its truth table, which is `editable || selection`.

- [ ] **Step 2: Run tests to verify they fail**

Run (from the repository root): `cargo test -p taskcascade-desktop`
Expected: FAIL.

- [ ] **Step 3: Intercept navigation and new windows in Rust**

- **Navigation.** `.on_navigation(|url| …)` returns `true` only for `Allow`.
  - `OpenExternally` calls `app.opener().open_url(url, None::<&str>)` and
    returns `false`.
  - `Block` logs and returns `false`.
- **New windows.** Ctrl+click, middle-click and `target=_blank` go through
  `.on_new_window(...)`, which applies the same classification and always
  denies the new window. If the resolved Tauri predates `on_new_window`,
  register WebView2's `NewWindowRequested` via `with_webview`, setting
  `Handled = true`.

No frontend change is needed. `react-markdown`'s default URL transform
already empties `javascript:` hrefs, and the shell is the backstop.

- [ ] **Step 4: Release WebView2 settings**

Release builds only (`cfg(not(debug_assertions))`):

- **Accelerator keys.** Set `AreBrowserAcceleratorKeysEnabled = false`. This
  removes the find bar, F5, Ctrl+R, Alt+Left and Ctrl+P, and leaves the
  editing keys alone.
- **Context menu.** Handle `ContextMenuRequested`. If
  `keep_default_context_menu(target.IsEditable, target.HasSelection)`, leave
  the event unhandled so WebView2 shows its default menu (cut, copy, paste and
  spell-check). Otherwise set `Handled = true` with no menu.

Prefer Tauri or wry builder options where the resolved version has them.
Otherwise use `with_webview` and `webview2-com` (`ICoreWebView2Settings3`,
`ICoreWebView2_11::add_ContextMenuRequested`). Devtools stay debug-only.

- [ ] **Step 5: Run the gate**

Run `npm run build` from `frontend/` first; release `build.rs` requires
`frontend/dist`.
Run (from the repository root):
`cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo clippy -p taskcascade-desktop --release --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Human verification (recommended; scratch data only)**

Link checks run in the debug window (`.\scripts\start.ps1 -Desktop`).
Keyboard, context-menu and browser-extras checks need the release build:

1. Run `npm run build` and `cargo build -p taskcascade-desktop --release`.
2. Run `target\release\taskcascade-desktop.exe` with an absolute scratch
   `TASKCASCADE_DATA_DIR` and `TASKCASCADE_CREATE_DATABASE=1`.
3. Restore the environment afterwards.

**Links.** Create a scratch task whose description contains:

- `[web](https://example.com)`;
- `[mail](mailto:someone@example.com)`;
- `[rel](notes.md)`;
- `[js](javascript:alert(1))`;
- `[file](file:///C:/Windows)`;
- `![img](https://upload.wikimedia.org/wikipedia/commons/4/47/PNG_transparency_demonstration_1.png)`;
- `![http img](http://example.com/x.png)`.

In Preview:

1. The https link opens the system browser, and the window stays on the app.
2. The mailto link opens the mail handler.
3. The relative, javascript and file links do nothing.
4. Ctrl+click and middle-click on the https link open the browser, and no
   second window appears.
5. The https image renders and the http image does not.

**Keyboard, menu and extras (release build).**

- Every row of the `docs/RequirementSpecification.md` keyboard table works:
  Ctrl+N, Enter, Ctrl+Up, Ctrl+Down, Delete, Ctrl+F and Escape. Ctrl+F
  focuses the app's search, with no find bar.
- F5, Ctrl+R, Alt+Left, Ctrl+P and F12 do nothing.
- Right-clicking in the title, description and scratchpad fields shows
  cut/copy/paste, and spell-check suggestions on a misspelt word.
- Right-clicking selected text in a Preview shows copy.
- Right-clicking anywhere else shows no menu.
- Drag-and-drop to Archive, Deleted and All tasks still works.

If F5/Ctrl+F/Ctrl+P still work on the first page but not after navigation,
the fallback is to reload the root once after applying the settings
(reload is allowed by `classify`).

- [ ] **Step 7: Commit**

Headline: `Plan.DesktopWindow Phase 6: keep links and browser extras out of the window`.

---

### Phase 7: No lost edits — the save coordinator and the close handshake

**Current behaviour and its four faults.** Today `TaskEditor`
(`frontend/src/App.tsx:723-746`) owns its draft in `useState(task)`, debounces
saves by 450 ms, and clears the timer on unmount. `updateTask`
(`App.tsx:270`) catches failures and only shows a toast. The backend's
`PUT /api/tasks/{id}` replaces all four editable fields and rejects a blank
title (`backend/src/app.rs:354-363`). That causes four faults:

1. Switching tasks, changing view or closing within 450 ms drops the edit.
2. A failed save is reported but its draft is lost.
3. A fast A → B → A reselection remounts A from stale application state, so a
   later edit writes old fields back.
4. Overlapping saves of one task have no ordering.

This phase moves draft ownership into a tested coordinator and makes every
lifecycle action and the window close depend on confirmed persistence.

**Files:**

- Modify:
  - `frontend/package.json`: the `vitest` dev-dependency,
    `"test": "vitest run"`, and `vitest.config.ts` added to the `check` and
    `fmt` file lists.
  - `frontend/src/api.ts`: in-flight tracking, `settle`, `reportClose`, and
    typed `ApiError`.
  - `frontend/src/App.tsx` and `frontend/src/styles.css`.
  - `desktop/src/launch.rs`, `desktop/src/bridge.rs` (in-flight counter and
    admission stop), and `desktop/src/lifecycle.rs`.
- Create:
  - `frontend/vitest.config.ts` (node environment);
  - `frontend/src/saveCoordinator.ts` and
    `frontend/src/saveCoordinator.test.ts`;
  - `frontend/src/closeFlow.ts` and `frontend/src/closeFlow.test.ts`;
  - `frontend/src/api.test.ts` and `frontend/src/shell.ts`;
  - `desktop/src/close_flush.rs`.

**Interfaces produced:**

```ts
// api.ts
export class ApiError extends Error { status: number }        // thrown by request() for non-2xx
export function settle(): Promise<void>;                      // resolves once no request() is in flight
export function reportClose(outcome: "saved" | "failed" | "discarded"): Promise<void>; // POST /desktop/close-ready?outcome=

// saveCoordinator.ts — pure: no React, injected send and timers
export type Fields = Pick<Task, "title" | "projectId" | "description" | "scratchpad">;
export type SaveStatus = "clean" | "pending" | "saving" | "failed";
export type SaveOutcome = { ok: true } | { ok: false; error: Error };
export type SaveCoordinator = {
  draft(taskId: string): Fields | undefined;                  // survives editor unmount
  status(taskId: string): SaveStatus;
  error(taskId: string): Error | undefined;
  edit(taskId: string, fields: Fields): boolean;              // false (and no change) while frozen
  flush(taskId: string): Promise<SaveOutcome>;                // ok only when the newest revision is confirmed
  flushAll(): Promise<SaveOutcome>;                           // every dirty task; first failure wins
  afterSaved<T>(taskId: string, action: () => Promise<T>): Promise<{ ok: true; value: T } | { ok: false; error: Error }>;
  freeze(): void; unfreeze(): void; frozen(): boolean;
  discardAll(): void;                                         // drops every draft — only for "close and discard"
  subscribe(listener: () => void): () => void; version(): number; // for useSyncExternalStore
};
export function createSaveCoordinator(options: {
  send: (taskId: string, fields: Fields) => Promise<Task>;    // api.updateTask
  onSaved: (task: Task) => void;                              // App merges the server task into data
  delayMs: number;                                            // 450
  timers?: { set: typeof setTimeout; clear: typeof clearTimeout };
}): SaveCoordinator;

// closeFlow.ts — pure orchestration, injected effects
export type CloseResult = "saved" | "failed";
export function runCloseFlow(deps: {
  coordinator: SaveCoordinator; settle: () => Promise<void>;
  report: (o: "saved" | "failed") => Promise<void>;
  showFailure: (error: Error) => void; setClosing: (closing: boolean) => void;
  timeoutMs: number;                                          // 10 000
}): Promise<CloseResult>;

// shell.ts — the one shell contract
export function registerBeforeClose(handler: () => Promise<CloseResult>): () => void; // sets/clears window.taskcascadeBeforeClose
```

Coordinator semantics:

- **Revisions.** Each task has a revision counter. `edit` stores the new
  fields as the draft, bumps the revision and (re)arms the debounce.
- **One request per task.** When the debounce fires, or `flush` is called,
  the coordinator sends the newest draft only if no request is in flight for
  that task. If one is in flight, the newest draft is sent when it settles.
  Older unsent drafts are never sent: the newest coalesces them.
- **Confirmation.** A successful response for revision *r* calls `onSaved`.
  It clears the draft only if *r* is still the newest revision. A newer draft
  stays authoritative, and its own send follows.
- **Failure.** A failure sets status `failed` and keeps the draft and the
  error. It sends nothing more until the next `edit` or `flush` (Retry).
- **Flush results.** `flush` resolves `ok` only once the newest revision is
  confirmed. If the request carrying the newest revision fails, `flush`
  resolves `{ ok: false }`.
- **Gating.** `afterSaved` runs `action` only after an `ok` flush.

```rust
// close_flush.rs
pub const CLOSE_ACK_PATH: &str = "/desktop/close-ack";
pub const CLOSE_READY_PATH: &str = "/desktop/close-ready";       // ?outcome=saved|failed|discarded|error|no-page
pub const EVAL_PROBE_PATH: &str = "/desktop/eval-ok";
pub const BEFORE_CLOSE_SCRIPT: &str = "(async () => { \
  await fetch('/desktop/close-ack', { method: 'POST' }); \
  if (typeof window.taskcascadeBeforeClose !== 'function') { await fetch('/desktop/close-ready?outcome=no-page', { method: 'POST' }); return; } \
  try { await window.taskcascadeBeforeClose(); } catch (e) { await fetch('/desktop/close-ready?outcome=error', { method: 'POST' }); } })();";
pub const ACK_DEADLINE: Duration = Duration::from_secs(3);
pub const FLUSH_BACKSTOP: Duration = Duration::from_secs(20);
pub enum Outcome { Saved, Failed, Discarded, Error, NoPage }
pub enum CloseState { Open, AwaitingAck, Flushing, Asking, Closing }
pub enum CloseStep {
    StartHandshake,   // prevent close, eval BEFORE_CLOSE_SCRIPT, arm ACK_DEADLINE
    Prevent,          // prevent close, nothing else
    ArmBackstop,      // ack received: arm FLUSH_BACKSTOP
    StayOpen,         // explicit failure: window stays, timers disarmed
    AskCloseAnyway,   // native Yes/No: close without confirmation?
    BeginShutdown(&'static str), // reason logged: "saved", "discarded", "no page answered", "closed without confirmation"
    Ignore,
}
pub struct CloseCoordinator { /* Mutex<CloseState>, attempt counter so stale timers are ignored */ }
impl CloseCoordinator {
    pub fn close_requested(&self) -> CloseStep;               // Open → AwaitingAck: StartHandshake; AwaitingAck/Flushing/Asking → Prevent; Closing → Ignore (let it close)
    pub fn ack(&self, attempt: u64) -> CloseStep;             // AwaitingAck → Flushing: ArmBackstop
    pub fn outcome(&self, outcome: Outcome) -> CloseStep;     // Saved/Discarded/NoPage → Closing: BeginShutdown; Failed → Open: StayOpen; Error → Asking: AskCloseAnyway
    pub fn ack_deadline(&self, attempt: u64) -> CloseStep;    // still AwaitingAck(attempt) → Closing: BeginShutdown("no page answered"); else Ignore
    pub fn backstop(&self, attempt: u64) -> CloseStep;        // still Flushing(attempt) → Asking: AskCloseAnyway; else Ignore
    pub fn answer(&self, close_anyway: bool) -> CloseStep;    // Asking → Closing (BeginShutdown) or Open (StayOpen)
}
pub fn router(coordinator: Arc<CloseCoordinator>, events: tokio::sync::mpsc::Sender<CloseEvent>) -> Router;
// bridge.rs
pub struct InFlight { /* AtomicUsize + Notify + admitting flag */ }
impl InFlight {
    pub fn track(&self) -> Option<InFlightGuard>;            // None once admission stopped → bridge answers 503 JSON
    pub fn stop_admitting(&self);
    pub async fn wait_idle(&self, timeout: Duration) -> bool;
}
```

The page may also report `saved` or `discarded` while the state is `Open`,
from its own close dialog. The coordinator treats that report as the user's
explicit close: `Open → Closing`.

- [ ] **Step 1: Write the failing tests**

Frontend (`npm test`, vitest, fake timers and controllable promises):

`saveCoordinator.test.ts`:

- **Debounce.** `edit` then 450 ms → exactly one `send` with the latest
  fields; `flush` sends immediately and the timer does not send again.
- **Validation failure (400) and database failure (500)**, each:
  - `flush` resolves `{ ok: false }`, status is `failed`, and the draft and
    error survive;
  - `afterSaved(id, complete)` never calls `complete`;
  - a later `flush` (Retry) whose `send` succeeds resolves `ok` with **exactly
    one** additional `send`, and the draft clears.
- **A → B → A before A's save resolves.**
  - Edit A's description (rev 1) and `flush(A)` with the send held.
  - "Switch" to B, then back to A: `draft(A)` still returns rev 1's fields.
  - Edit A's title (rev 2) from that draft. Release rev 1's send: `onSaved` is
    called, but `draft(A)` still holds rev 2.
  - Then `send` is called once more with rev 2, containing **both** the new
    description and the new title.
  - After it resolves, the last persisted fields equal the newest draft.
- **Overlapping saves.**
  - Hold the first send, edit again, and `flush`. No second `send` starts
    while the first is held.
  - After release, exactly one more `send` carries the newest fields.
  - `flush` resolves `ok` only after that second send; the order of sends is
    rev 1, then newest.
- **Stale response.** A response for an older revision never clears or
  replaces a newer draft.
- **Frozen.** While frozen, `edit` returns `false` and changes neither the
  draft, the revision nor the timers. After `unfreeze`, edits work again.
- **Discard.** `discardAll` drops drafts and pending timers, and sends
  nothing.

`closeFlow.test.ts`:

- **Edit during close.** Hold an in-flight save, run `runCloseFlow`, and
  attempt `edit` on the title, description and scratchpad before releasing.
  - Each edit is refused.
  - `setClosing(true)` happened **before** the first flush call (synchronously
    at entry).
  - After release and success, `report("saved")` is called exactly once.
- **Failure.** A failing save leads to `report("failed")`, `showFailure`,
  `setClosing(false)` and an unfrozen coordinator with the draft intact.
  `report("saved")` is never called.
- **Timeout.** A save that never settles within `timeoutMs` produces
  `report("failed")` with a "did not finish" error, and the draft is intact.
- **Retry.** Retry after failure with a now-succeeding `send` leads to
  `report("saved")` and exactly one additional `send`.

`api.test.ts` (`vi.stubGlobal("fetch", …)`):

- `settle()` waits for two in-flight requests, including one started while
  settling. It resolves immediately when idle, and resolves even when a
  request rejects.
- A non-2xx response throws `ApiError` with the status and the server
  `message`.

Desktop (`cargo test -p taskcascade-desktop`):

- **`CloseCoordinator`:**
  - the first close starts the handshake, and repeated close requests only
    prevent;
  - ack, then `Failed`, gives `StayOpen`. The original ack deadline and
    backstop timers firing afterwards return `Ignore`, so the timer does not
    override an explicit failure;
  - no ack within the deadline gives `BeginShutdown("no page answered")`;
  - ack then silence until the backstop gives `AskCloseAnyway`. A "No" answer
    returns to `Open`, and a later close starts a new attempt whose stale
    timers are ignored;
  - `Error` gives `AskCloseAnyway`;
  - `Saved`, `Discarded` and `NoPage` give `BeginShutdown`;
  - `Saved` or `Discarded` while `Open` (the page's dialog) gives
    `BeginShutdown`.
- **`InFlight`:**
  - `wait_idle` returns `true` after the last guard drops, and `false` on
    timeout while a guard is held;
  - after `stop_admitting`, `track` returns `None` and the bridge answers 503;
  - `/desktop/*` requests are never counted.
- **`lifecycle`, held request.** Hold an `/api` guard and shut down with a
  500 ms deadline. The report has `drained == false` and
  `exhausted_at == Some("drain")`, it returns within budget, and pool closure
  is skipped. A separate test with a 3 s deadline times out at the 2 s drain
  cap and closes the pool within the remaining budget.

- [ ] **Step 2: Run tests to verify they fail**

Run (from `frontend/`): `npm install` (adds vitest), then `npm test`. Run (from
the repository root): `cargo test -p taskcascade-desktop`.
Expected: FAIL.

- [ ] **Step 3: Frontend — coordinator, editor, lifecycle gating**

- **`api.ts`.**
  - `request()` adds its promise to a module-level `Set` synchronously,
    before the first `await`, and removes it in `finally`.
  - It throws `ApiError` (status plus server message).
  - `settle()` loops `Promise.allSettled` until the set is empty.
  - `reportClose(outcome)` posts to `/desktop/close-ready?outcome=…`. It lives
    here because it is a fetch.
- **`App`.**
  - Creates one coordinator in a ref, with `send: api.updateTask`,
    `onSaved` merging the returned task into `data` with a functional
    `setData`, and `delayMs: 450`.
  - Subscribes with `useSyncExternalStore(coordinator.subscribe, coordinator.version)`.
  - The current `updateTask` with its catch-and-toast is removed.
- **`TaskEditor`.**
  - Holds no draft state. It renders `coordinator.draft(task.id) ?? fieldsOf(task)`,
    and each input calls `coordinator.edit(task.id, {...shown, ...patch})`.
  - Inputs are `readOnly` while `coordinator.frozen()`.
  - Unmount calls `void coordinator.flush(task.id)`, so saving starts at once.
    Failures surface through the coordinator state; the draft survives, so
    returning to the task shows it.
  - The `.autosave` line reflects `status(task.id)`: "Saved automatically",
    "Saving…", or "Not saved: <message> · Retry". The Retry button calls
    `flush`, and the line reuses the existing toast and danger tokens (no new
    colour).
  - A failed task that is no longer shown raises the existing toast with the
    task title, the message and a Retry action. This is a small extension of
    the toast.
- **Lifecycle gating.** These actions run through
  `coordinator.afterSaved(task.id, () => api.…)`:
  - `complete` (the Delete key and dropping on Archive);
  - `moveToTrash`;
  - any `dropAction` on an active task.

  On `{ ok: false }` the action does not run, the task stays where it is, and
  the toast reads "Not completed/deleted — the latest edit to “<title>” could
  not be saved: <message>". Restore and undelete act on read-only tasks with
  no draft and need no gating.
- **Close flow.**
  - A mount effect calls
    `registerBeforeClose(() => runCloseFlow({ … timeoutMs: 10_000 }))`.
  - `setClosing(true)` shows an unobtrusive "Saving before close…" overlay and
    makes App's mutation handlers, keyboard shortcuts, drag sources and
    dialogs inert.
  - `runCloseFlow`:
    1. calls `setClosing(true)` and `coordinator.freeze()` synchronously;
    2. awaits `flushAll()` and then `settle()`, bounded by `timeoutMs`;
    3. on success, calls `report("saved")`;
    4. on failure, calls `report("failed")`, unfreezes, calls
       `setClosing(false)`, and `showFailure` opens the close-failure dialog.
- **Close-failure dialog** (existing `Dialog` component). It reads "Your
  changes to “<title>” could not be saved: <message>", with three buttons:
  - **Retry and close** runs the close flow again; on success the flow
    reports `saved`, which closes from `Open`.
  - **Close and discard unsaved changes** calls `coordinator.discardAll()` and
    then `api.reportClose("discarded")`.
  - **Keep editing** closes the dialog.

The React flow remains input → draft (coordinator) → API mutation → refreshed
state → render.

- [ ] **Step 4: Shell — handshake, admission, bounded shutdown**

- **Close requests.** In the run-event callback, handle
  `RunEvent::WindowEvent { label: "main", event: WindowEvent::CloseRequested { api, .. } }`
  by calling `close_requested()`:
  - `StartHandshake`: `api.prevent_close()`,
    `window.eval(BEFORE_CLOSE_SCRIPT)`, and arm `ACK_DEADLINE` for this
    attempt.
  - `Prevent`: `api.prevent_close()`.
  - `Ignore` while `Closing`: the shutdown task is already running.
- **Events.** The routes `/desktop/close-ack` and
  `/desktop/close-ready?outcome=` are composed in `launch::compose` ahead of
  the asset fallback, and answer 204. They and the timers feed one event
  channel.
- **Driver.** A driver task on the runtime applies each `CloseStep`:
  - `ArmBackstop`: arm the 20-second backstop.
  - `StayOpen`: nothing further; the window stays open.
  - `AskCloseAnyway`: show a native `MessageBoxW` Yes/No on the main thread
    via `run_on_main_thread`: "TaskCascade could not confirm that your latest
    edits were saved. Close anyway? Unsaved changes may be lost." Feed the
    answer back through `answer()`.
  - `BeginShutdown(reason)`: log the reason, call `in_flight.stop_admitting()`,
    then `app_handle.exit(0)`. `RunEvent::Exit` runs
    `Lifecycle::shutdown(SHUTDOWN_DEADLINE)`, which now drains
    (`wait_idle(min(2 s, remaining))`) before closing the pool, all within the
    one deadline from Phase 4.
- **Logging.** Close outcomes are logged in distinct words:
  - "close: page confirmed all edits saved";
  - "close: user discarded unsaved changes";
  - "close: no page answered";
  - "close: closed without confirmation".

  These are followed by the shutdown report.
- **Startup eval self-test.** When the page has loaded, the shell evals
  ``fetch('/desktop/eval-ok', { method: 'POST' })``. The log records
  "eval self-test ok", or a loud warning if the call has not arrived within
  5 s. This proves the close handshake can run under the page's CSP.
- Alt+F4 and taskbar "Close window" take the same path.

- [ ] **Step 5: Run the gates**

Run (from `frontend/`): `npm test && npm run check && npm run fmt`. Run (from
the repository root):
`cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`.
Expected: PASS.

- [ ] **Step 6: Human verification (recommended; scratch development window)**

At 450 ms a slow hand passes every check without exercising anything, so run
checks 1–6 on a throwaway build with the coordinator's `delayMs` raised to
5000 ms (not committed). Then repeat checks 1 and 2 at 450 ms.

1. **Type, close immediately, reopen.** Type in a description and close at
   once. Reopen: the text is there, and the log says "page confirmed all edits
   saved". Repeat with Alt+F4 and with the scratchpad tab.
2. **A → B → A.** Type in A's description, click B, click A again at once, and
   edit A's title. Reopen the app: both the description and the title changes
   are saved.
3. **Blank title blocks completion.** Clear a task's title, click its row, and
   press Delete.
   - The task stays active, a toast explains that the edit could not be saved,
     and the editor shows "Not saved … Retry".
   - Type a title, and the save succeeds.
   - Press Delete again, and the task is archived with the new title.
   - Repeat with a drag to Deleted.
4. **Blank title blocks close.**
   - Clear a title and close the window. It stays open with the close-failure
     dialog, and the log says the page reported failure; no 3-second close
     happens.
   - **Keep editing:** type a title, close again, and the window closes and
     the title is saved.
   - Repeat, and choose **Close and discard unsaved changes**. The window
     closes and the old title is still stored. The log says "user discarded
     unsaved changes".
5. **Edits during close.** In devtools, set network throttling to a slow
   profile. Type, close, and try to type in the title, description and
   scratchpad while "Saving before close…" shows. The fields do not change.
   After the window closes and is reopened, the text typed before closing is
   saved.
6. **Page never answers.** In devtools, run
   `window.taskcascadeBeforeClose = () => new Promise(() => {})` and close.
   - After about 20 s a native "Close anyway?" question appears.
   - "No" keeps the window open.
   - Closing again and answering "Yes" closes it, and the log says "closed
     without confirmation".
7. **Shutdown report.** After any normal close, the log's shutdown report says
   drained and pool closed, with no exhausted stage.

- [ ] **Step 7: Commit**

Headline: `Plan.DesktopWindow Phase 7: confirm saves before lifecycle actions and window close`.

---

### Phase 8: The installed copy — Lars switches over

**Files:**

- Create: `scripts/install.ps1` and `scripts/tests/rehearse-install.ps1`.
- No Rust changes. Release embedding and the asset-policy guard landed in
  Phase 3.

- [ ] **Step 1: Write `scripts/install.ps1`**

**Parameters:**

- `-InstallDir` (default `$env:LOCALAPPDATA\Programs\TaskCascade`);
- `-ShortcutPath` (default
  `[Environment]::GetFolderPath('Programs')\TaskCascade.lnk`);
- `-NoShortcut`;
- `-DataDir` (default `$env:LOCALAPPDATA\TaskCascade`, used for the existence
  check and `-InitDatabase` only);
- `-SkipBuild`, `-SkipInstall` and `-InitDatabase`.

A comment names `desktop/src/data_location.rs` as the defining default.

**Steps:**

1. **Guards.**
   - Refuse if the legacy `.local\data\taskcascade.sqlite*` exists; point to
     `migrate-data.ps1`.
   - Refuse with "Close TaskCascade first" if a `taskcascade-desktop` process
     is running from `-InstallDir` (`Get-Process | Where Path -eq`).
   - Throw if `LOCALAPPDATA` is unset.
2. **Build** (unless skipped). `npm install` and `npm run build` in
   `frontend\`, then `cargo build -p taskcascade-desktop --release` from the
   root.
3. **Copy.** Copy `target\release\taskcascade-desktop.exe` into `-InstallDir`
   under a temporary name, then rename it over the old file. Write
   `install-info.txt`: `git rev-parse HEAD`, whether the tree is dirty, and
   the UTC timestamp.
4. **Shortcut** (unless `-NoShortcut`). Create or replace the `.lnk` at
   `-ShortcutPath` through `WScript.Shell`, with the installed executable as
   target and icon, and `-InstallDir` as working directory. There is no
   desktop shortcut and no `PATH` change.
5. **Database check.** If `<DataDir>\taskcascade.sqlite` is missing:
   - with `-InitDatabase`, launch the installed executable once with
     `TASKCASCADE_CREATE_DATABASE=1`. When `-DataDir` is not the default, also
     set `TASKCASCADE_DATA_DIR=<DataDir>`. Both are scoped to that child (set,
     `Start-Process`, restore);
   - otherwise, print that TaskCascade will refuse to start and how to create
     the database.

   `-InitDatabase` with an existing database does nothing and says so.
6. **Summary.** Print the installed path, `install-info`, the shortcut path
   (or "none"), the database path and whether it exists, and "Re-run this
   script to update; rebuilding in the repository never changes the installed
   copy". Uninstalling means deleting `-InstallDir` and the shortcut; the data
   stays.

- [ ] **Step 2: Write `scripts/tests/rehearse-install.ps1` (synthetic only)**

`-Root <temp dir>` follows the same rules as the Phase 1 rehearsals: it must
be empty or absent and outside the real-data locations, and only its contents
are deleted at the end.

1. **Sentinel.** Record whether the real
   `[Environment]::GetFolderPath('Programs')\TaskCascade.lnk` exists, and if
   so its SHA-256 and its target as read through `WScript.Shell`, without
   modifying it.
2. **Install.** Run
   `install.ps1 -InstallDir <root>\install -ShortcutPath <root>\shortcut\TaskCascade.lnk -DataDir <root>\data -InitDatabase`.
   - The files and `install-info.txt` exist.
   - The test shortcut's target is `<root>\install\taskcascade-desktop.exe`.
   - `<root>\data\taskcascade.sqlite` was created.
   - The executable size reflects the embedded frontend.
3. **Running copy refused.** Start the rehearsal copy with
   `TASKCASCADE_DATA_DIR=<root>\data`. A second `install.ps1` with the same
   `-InstallDir` refuses. Stop the copy.
4. **Self-contained.** Rename `frontend\dist` temporarily and launch the
   rehearsal copy with the same scratch environment. The UI still appears.
   Restore `dist`.
5. **`-NoShortcut`.** An install into `<root>\install2` with `-NoShortcut`
   creates no `.lnk` anywhere under the root.
6. **Cleanup and sentinel.** Remove only `<root>`. The real Start-menu
   shortcut's existence, hash and target equal step 1's record.

- [ ] **Step 3: Run the gates and the rehearsal**

Run (from the repository root):
`cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`.
Then run `scripts\tests\rehearse-install.ps1` with a fresh root.
Expected: PASS.

- [ ] **Step 4: Lars (required): install and switch daily use**

1. Stop `start.ps1 -RealData` and close its browser tab.
2. Run `.\scripts\install.ps1`.
3. Start **TaskCascade** from the Start menu.
   - The title is "TaskCascade" (no "(dev)").
   - All tasks, projects, the archive and the trash are present, and the
     counts match the Phase 1 migration summary.
   - The log names the real database and "eval self-test ok".
4. `Get-NetTCPConnection -State Listen -OwningProcess (Get-Process taskcascade-desktop).Id`
   returns nothing.
5. Clicking the shortcut again focuses the window.
6. `.\scripts\start.ps1 -Desktop` opens a separate "TaskCascade (dev)" window
   on scratch data at the same time.
7. A rebuild in the repository
   (`cargo build -p taskcascade-desktop --release`) succeeds, and the installed
   window is unaffected.
8. On real data, do Phase 7 checks 1 and 2 once, using a throwaway edit that
   is then reverted.

From now on the installed copy is the daily app.

- [ ] **Step 5: Commit**

Headline: `Plan.DesktopWindow Phase 8: install a self-contained copy with a Start-menu shortcut`.

---

### Phase 9: Remove browser support

**Files:**

- Delete: `backend/src/main.rs` and
  `scripts/tests/rehearse-real-data-snapshot.ps1`.
- Modify:
  - `backend/Cargo.toml`: remove `tower-http`, and trim tokio to the features
    the library and its tests use (no `net`). The package stays library-only,
    with its examples.
  - `frontend/vite.config.ts`: remove the `server.proxy` block and `loadEnv`,
    and keep `assetsInlineLimit: 0`.
  - `frontend/package.json`: remove the `dev` script.
  - `scripts/start.ps1`: rewritten as the development-window launcher.

- [ ] **Step 1: Write the failing check**

This repository search must return nothing outside `docs/`:

```powershell
git grep -nE "TcpListener|axum::serve|CorsLayer|ServeDir|TASKCASCADE_PORT|127\.0\.0\.1|5173|8080|RealData" -- backend desktop frontend/src frontend/vite.config.ts frontend/package.json scripts
```

Expected before the change: matches in `backend/src/main.rs`,
`frontend/vite.config.ts`, `scripts/start.ps1` and the snapshot rehearsal.

- [ ] **Step 2: Delete the server shell and its dependencies**

The 16 backend tests and the desktop tests must not need any of them.

- [ ] **Step 3: Rewrite `scripts/start.ps1`**

Parameters: `-Release`, `-SkipInstall` and `-SkipBuild`. There are no port
parameters, and `-RealData`, `-CreateRealDataSnapshot`, `-Desktop` and the
rehearsal parameters are gone.

1. **Legacy guard.** Keep the legacy `.local\data` guard.
2. **Running copy.** Refuse if `target\<profile>\taskcascade-desktop.exe` is
   running: "close the development window first".
3. **Build.** `npm install` unless `-SkipInstall`, `npm run build`, then
   `cargo build -p taskcascade-desktop [--release]`.
4. **Run.** Set `TASKCASCADE_DATA_DIR` to the absolute `.local\dev-data` and
   `TASKCASCADE_CREATE_DATABASE=1`, then
   `Start-Process -Wait -NoNewWindow`. Restore the environment in `finally`
   and pass through the exit code.
5. **Messages.** Print the database path and the log path, "Close the window
   to stop (Ctrl+C hard-kills it without saving a pending edit)", and, for
   debug builds, "After npm run build in frontend\, press F5 in the window to
   reload the interface".

Delete `.local\real-run\` (the interim snapshot) after confirming nothing runs
from it. Lars has switched in Phase 8.

- [ ] **Step 4: Run the gates**

Run (from the repository root):
`cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`.
Run (from `frontend/`): `npm test && npm run check && npm run fmt`. Run the
Step 1 search, which must return no matches, and rerun
`scripts\tests\rehearse-migrate-data.ps1` and
`scripts\tests\rehearse-install.ps1` with fresh roots.
Expected: PASS.

- [ ] **Step 5: Human verification (recommended)**

1. `.\scripts\start.ps1` opens the debug development window on
   `.local\dev-data`.
2. Change a visible string, run `npm run build` in `frontend\`, and press F5
   in the window: the change appears.
3. `.\scripts\start.ps1 -Release` opens a release "TaskCascade (dev)" window
   on scratch data, with no console and no browser extras.
4. The installed copy is unaffected.

- [ ] **Step 6: Commit**

Headline: `Plan.DesktopWindow Phase 9: remove the browser run path and the loopback server`.

---

### Phase 10: Documentation reconciliation and decision log

**Files:**

- Modify: `README.md`, `Agents.md`, `docs/Design.HighLevel.md`,
  `docs/RequirementSpecification.md`, `docs/Plans/Plan.agent-cli-access.md`,
  `.vscode/tasks.json`, `docs/DecisionLog.md` (append only).

- [ ] **Step 1: `README.md`**

Replace "Run locally" with:

- **Prerequisites:** Rust, Node.js/npm, and the WebView2 runtime (present on
  Windows 11).
- **Install:** `.\scripts\install.ps1` and the Start-menu shortcut; updating
  by re-running the script; uninstalling.
- **Where data lives:** `%LOCALAPPDATA%\TaskCascade\taskcascade.sqlite`.
  Back up by copying it while TaskCascade is closed. The database is never
  created implicitly (`-InitDatabase`).
- **Development:** `.\scripts\start.ps1` (or `-Release`), scratch data in
  `.local\dev-data`, F5 after `npm run build`, and `npm test`.
- **Older checkouts:** one line pointing to `scripts\migrate-data.ps1` for a
  checkout that still holds `.local\data`.

Remove the browser URL and port text. "MVP features" gains: "Runs as a desktop
window; links open in your browser; edits are confirmed saved before
completing, deleting or closing."

- [ ] **Step 2: `Agents.md`**

**Workflow:**

- The repository is a Cargo workspace (`backend/`, `desktop/`) with one
  `target/`. Cargo commands run from the repository root.
- Backend-only work runs `cargo test -p taskcascade-backend` and
  `cargo clippy -p taskcascade-backend --all-targets -- -D warnings`. Changes
  to `desktop/` or a workspace manifest widen to `--workspace`, because
  WebView2 builds slowly. Finish with `cargo fmt --all`.
- Frontend changes run `npm test`, `npm run check` and `npm run fmt` from
  `frontend/`.
- `scripts/start.ps1` opens the development window on scratch data, and
  `scripts/install.ps1` updates the installed copy.
- Never point development, test or rehearsal runs at
  `%LOCALAPPDATA%\TaskCascade`; it holds Lars's real tasks. Script rehearsals
  in `scripts/tests/` use synthetic fixtures under a temporary root.

**Architecture:** rewrite the first bullet to:

> The application is a local-first desktop app: SQLite is the data of record;
> the Tauri shell in `desktop/` serves the UI and the axum API in-process
> through one custom URI scheme — no TCP listener, no browser. The frontend
> uses relative `/api` requests and no Tauri JavaScript API.

Add two bullets:

- The `backend` crate is a library (API, SQL, domain). Shell, path, window and
  WebView2 concerns live in `desktop/`.
- Task drafts are owned by the save coordinator (`frontend/src/saveCoordinator.ts`).
  Lifecycle actions and window close proceed only after a confirmed save.

- [ ] **Step 3: `docs/Design.HighLevel.md`**

- If a Tauri capability file is ever added, it must not grant opener permissions
  (e.g. `opener:default`), which would give page script a URL-opening command.
- **Architecture table:**
  - **Desktop shell:** Rust + Tauri 2 (WebView2), responsible for the window,
    the scheme bridge, frontend assets, the data location, single instance,
    the close handshake and the bounded shutdown.
  - **Local backend:** Rust + axum as a library, responsible for the REST API,
    the ordering invariant and persistence.
  - **Storage:** SQLite.
  - **UI:** Vite + React + TypeScript, built and embedded in release builds.
  - **Markdown.**
- **Replace the Vite proxy paragraph** with:
  - the in-process scheme (one origin; the CSP is emitted by the shell);
  - data location and identity (canonical comparison, installed copy versus
    development launcher, the creation opt-in);
  - links opening in the system browser;
  - remote images over https only;
  - the release context-menu policy;
  - window state restored at startup.
- **Interaction model:** "Edits are kept as drafts by a save coordinator that
  writes after a short idle delay, one request per task at a time. Switching
  away starts the save immediately. Completing, deleting and closing wait for
  a confirmed save. A failed save keeps the draft and offers Retry, and
  closing then offers 'close and discard unsaved changes' as the only way to
  drop an edit."

- [ ] **Step 4: `docs/RequirementSpecification.md` (inside its outer ```` ```markdown ```` fence)**

Replace the Technology section's body with:

```markdown
Application:

- Desktop window (Rust, Tauri 2 on WebView2) with a web-technology interface (React, TypeScript)

Storage:

- Local storage only (one SQLite file in the user's local application data)

No server, network port, hosting or account required.
```

Leave the other sections untouched. "Window state" under Persistence is now
actually met.

- [ ] **Step 5: `docs/Plans/Plan.agent-cli-access.md`**

Insert one line directly under the title:

> **Parked (Plan.DesktopWindow):** TaskCascade is now a desktop app that
> opens no port, so this plan's loopback client cannot reach it; the plan is
> parked, and any CLI would need a separate console binary because the
> desktop executable is a GUI-subsystem program.

Change nothing else.

- [ ] **Step 6: `.vscode/tasks.json`**

- Backend build, test and clippy run from `${workspaceFolder}` with
  `-p taskcascade-backend`.
- Add "Desktop build" (`cargo build -p taskcascade-desktop`), "Workspace
  clippy", "Frontend build" (`npm.cmd run build`) and "Frontend test"
  (`npm.cmd test`).
- "Start application" becomes "Start development window"
  (`./scripts/start.ps1`).
- Add "Install TaskCascade" (`./scripts/install.ps1`).
- "Check all" runs workspace test, workspace clippy, frontend test and
  frontend check.

- [ ] **Step 7: Check that no stale wording remains**

Search the docs for `loopback`, `5173`, `8080`, `browser`, `Vite proxy`,
`Documents/TaskCascade` and `RealData`. Every remaining hit must be
intentional: historical DecisionLog entries, the parked agent plan, or this
plan.

- [ ] **Step 8: Append the decision-log entries**

Append at the end of `docs/DecisionLog.md`, dated the day this phase lands. Do
not edit earlier entries.

```markdown
## YYYY-MM-DD — TaskCascade is a desktop application; browser support is removed

Decision: TaskCascade runs as a native desktop window (Tauri 2 on WebView2). The window loads the interface and the API through one custom URI scheme whose handler feeds each request into the axum router in-process; no TaskCascade process opens a TCP listener or port, and there is no browser run path. Page and API share one origin, so the frontend keeps relative `/api` requests and uses no Tauri JavaScript API or typed commands. The `backend` crate is a library (API, SQL, domain); the `desktop` crate is the only executable and also owns serving the built frontend and the content-security policy.
Context: A stand-alone window was wanted instead of a browser tab plus a background server. This supersedes the "web application" and "backend listens on loopback" parts of the 2026-07-10 "Local SQLite task stack" entry — its single-user, local-SQLite, copy-one-file-to-back-up commitments stand — and replaces the never-implemented single-binary browser packaging design (git 14f5e5f), which had chosen browser distribution over Tauri. Keeping a browser path behind a flag was rejected as two shells to maintain for one user.
Consequences: Interface changes are seen by rebuilding and reloading or reopening the window; there is no live reload. The permissive CORS layer and the Vite `/api` proxy are gone. Nothing outside the process can reach the API, so any external client — including the parked agent-access plan — needs a separate future decision. The policy forbids inline scripts and `data:` sources, so the frontend must ship no inline script and no bundler-inlined asset.

## YYYY-MM-DD — Real tasks live in local application data; the installed copy and the development launcher are separate

Decision: Lars's tasks live in `%LOCALAPPDATA%\TaskCascade\taskcascade.sqlite`, outside the repository and outside any synced folder. The daily app is an installed, self-contained copy (frontend embedded in the release binary) placed in `%LOCALAPPDATA%\Programs\TaskCascade` by `scripts/install.ps1` with a Start-menu shortcut; rebuilding in the repository never changes it. `scripts/start.ps1` opens a development window on scratch data in `.local/dev-data`. The database location is resolved once by filesystem identity — not by path spelling — and that one result decides both the window's identity and whether a missing database may be created: the real location runs as `se.pensjo.taskcascade`, anything else as `se.pensjo.taskcascade.dev` titled "TaskCascade (dev)", and directories nested in or containing the real one, or a database file that is the real one under another name, are refused at startup. Each copy is a single instance; a second launch focuses the first. No build creates the real database implicitly; a missing database is a native startup dialog naming the path, and creation needs `TASKCASCADE_CREATE_DATABASE=1`.
Context: The real tasks had accumulated in the repository's scratch file, where development runs and cleanups could touch them; a one-time verified migration moved them. Running the daily app from the build tree was rejected because a running executable blocks rebuilds and half-built work would become the daily app. A synced Documents folder was rejected because a live SQLite file and its sidecars under sync risk corruption. A second window on the same database would silently overwrite notes, because a task update writes the whole record and nothing refetches; comparing path strings would have let a differently spelled path to the real directory open it as a second, "development" copy.
Consequences: Updating the daily app means re-running the install script with TaskCascade closed. Only `start.ps1`'s default development mode refuses to start while the pre-migration `.local/data` database or its sidecars exists; no parameter redirects or disables the guard. `-CreateRealDataSnapshot` skips it because snapshot creation touches no task data; `-RealData` skips it but still refuses before migration because `<RealDataRoot>\taskcascade.sqlite` does not exist yet. The startup log and the failure dialog name the resolved database path. The default real path is defined in the desktop crate and repeated in the scripts; changing it means changing all of them. Script rehearsals use synthetic fixtures and never the real paths. Backups remain future work.

## YYYY-MM-DD — Inside the window: links leave, and edits are confirmed before they can be lost

Decision: The window only ever shows TaskCascade. Navigation is decided in the shell: `http`, `https` and `mailto` links open in the system's handlers, and every other target — relative links, `file:`, `javascript:` and other schemes — is blocked. The new window is always denied, while an `http`, `https` or `mailto` target still opens in the system handler. Remote images in notes render only over https. Release builds turn off WebView2's browser accelerator keys and show the default context menu only on editable text and selections; devtools exist only in debug builds. Task drafts are owned by a save coordinator that writes one request per task at a time, keeps a failed draft with its error and a Retry, and lets completing, deleting and closing proceed only after the latest draft is confirmed saved. Closing freezes editing, asks the page to confirm its saves, keeps the window open if a save fails, and then shuts down within one bounded deadline. The single deliberate exception to "no lost edits" is the user explicitly choosing "close and discard unsaved changes" after a failed save, so a persistent failure can never trap the user in the window.
Context: In a browser, the browser's own chrome handled links, reloads and closing; in a bare window, a followed link would replace the app, and a close or lifecycle action within the save delay — or after a rejected save such as a blank title — would silently lose the latest text. Treating "the request finished" as "the edit is saved" was rejected because it acknowledged lost edits.
Consequences: The frontend's contract with the shell is one global close hook plus two close-report routes; changing how the shell closes means keeping them. If no live page answers within a few seconds the window closes, because nothing could save from a page that never responds; a page that answered but never reports asks the user before closing. Plain-http images in notes no longer render. Ending the process abruptly (task kill, Ctrl+C in a development console, operating-system shutdown) can still lose an edit younger than the save delay; committed data is unaffected.
```

- [ ] **Step 9: Full verification**

Run (from the repository root):
`cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`.
Run (from `frontend/`): `npm test && npm run check && npm run fmt`. Run the
Phase 9 search and the two remaining rehearsal scripts with fresh roots.
Expected: all pass with no formatting diffs.

- [ ] **Step 10: Lars (required): final by-eye pass**

1. The installed copy, started from the Start menu, opens his real tasks.
2. Closing a moment after typing keeps the edit.
3. A blank title blocks closing and offers Retry or discard.
4. Links open the browser.
5. No browser opens at any point.
6. `.\scripts\start.ps1` opens the development window beside the installed
   one.
7. The README's instructions match what he sees.

- [ ] **Step 11: Commit**

Headline: `Plan.DesktopWindow Phase 10: document the desktop app and record the decisions`.

---

## Documents this work updates

| Document | Why |
| --- | --- |
| `docs/DecisionLog.md` | Three appended entries. (1) Desktop architecture and browser removal, superseding part of the 2026-07-10 entry and the unimplemented packaging design. (2) The data location, the installed copy versus the development launcher, identity resolved by filesystem identity, and single instance. (3) Links, CSP images, the context menu, the save coordinator, the close handshake, and the explicit discard exception. |
| `docs/Design.HighLevel.md` | Shell, in-process scheme, data location and identity, links, images, the save and close model, and window state. |
| `docs/RequirementSpecification.md` | The Technology section: a desktop window, local SQLite, and no server, port or account. |
| `README.md` | Install, data location, development launcher, `npm test` and a migration pointer replace the browser instructions. |
| `Agents.md` | Workspace commands (`-p` versus `--workspace`), `npm test` in the frontend gate, the launch and install scripts, the real-data and rehearsal rules, the rewritten Architecture bullet, and the save-coordinator rule. |
| `docs/Plans/Plan.agent-cli-access.md` | A one-line parked note only. |
| `.vscode/tasks.json` | Tasks for workspace, desktop, frontend test, the development window and install. |
| `scripts/start.ps1`, `scripts/migrate-data.ps1` (new), `scripts/install.ps1` (new), `scripts/tests/*` (new) | The launch, migration and install workflow and their synthetic rehearsals. |
| `desktop/icons/README.md` (new) | How the icon is regenerated from `frontend/public/favicon.svg`. |

The repository keeps no requirement tags, changelog or benchmark suite, and
has no version policy, so no such artefacts or version bump are coupled to
these phases. The workspace version starts at the backend's existing `0.1.0`.
`docs/VisualDesign.DarkTheme.md` needs no change: the native window background
mirrors `--canvas`, and the save-status line reuses existing tokens.

## Resolved questions

From the brief (decisions by Lars):

1. **Shell and transport:** Tauri 2; a custom-scheme handler feeding the axum
   router in-process; no port; relative fetch; no Tauri JavaScript API or
   typed commands.
2. **Browser support:** removed in this work, not kept behind a flag. Lars
   accepts rebuild-and-reload instead of live reload.
3. **Port for agents:** none for now. The agent CLI plan is parked, and its
   conflict with the desktop app is left open.
4. **Superseded commitments:** the 2026-07-10 "web application / loopback"
   wording and the unimplemented packaging design (14f5e5f) are superseded
   by new appended entries.
5. **Launch paths:** an installed copy for daily use, and `start.ps1` as the
   development launcher on scratch data. The installed executable is
   self-contained, is updated by re-running the install script, and can run
   at the same time as the development copy.
6. **Data:** the real tasks move from `.local/data` to
   `%LOCALAPPDATA%\TaskCascade\taskcascade.sqlite` with their sidecars, a
   safety copy and verification. The Documents copy is reported but not
   touched. A missing database is refused unless creation is opted into.
   `instances\` is left alone.
7. **Branch:** the work builds on `main`, and `feature/agent-cli` stays
   parked.

Answered after the first draft (2026-10-22 review round):

8. **Release context menu** (*Claude's call*). Keep WebView2's default menu on
   editable text and selections (cut, copy, paste, spell-check) and suppress
   it everywhere else in release builds. Implemented in Phase 6, decision 13.
9. **`vitest`** (*Lars's decision*). It is added, `npm test` joins the
   frontend gate, and Phase 10 records that in `Agents.md`. It carries the
   save-coordinator and close-flow tests: failed save, A → B → A, overlapping
   saves, and an edit during close.
10. **Interim daily path** (*Claude's call*). Keep the interim
    `start.ps1 -RealData` snapshot, hardened per review finding 6: it is
    created once from the verified Phase 1 commit, hash-verified on every
    launch, and never rebuilt or recopied by a launch.
11. **Identifiers** (*Claude's call*). `se.pensjo.taskcascade` and
    `se.pensjo.taskcascade.dev`, as written.
12. **Persistent save failure at close** (*Claude's call*). The window stays
    open with the error and a Retry. An explicit "Close and discard unsaved
    changes" choice ensures the user is never trapped. It is documented as the
    one deliberate exception to "no lost edits" (Phase 7, the third
    decision-log entry, `Design.HighLevel.md`).

All eight review findings in `docs/Review.desktop-window.md` are applied:

- #1 (Phase 7 coordinator, gating and close outcomes);
- #2 (drafts above the editor, per-task serialization);
- #3 (closing freeze);
- #4 (Phase 4 filesystem identity and alias rejection);
- #5 (installer `-ShortcutPath`/`-NoShortcut` and sentinel rehearsal);
- #6 (fixed, hash-verified snapshot);
- #7 (synthetic fixtures including WAL-only data, refusal of hot journals,
  forced-failure recovery, and the corrected journal-mode statement);
- #8 (one bounded shutdown including pool close and runtime teardown).

The brief's remaining "Settled by Claude" items are adopted unchanged. This
plan's own choices are listed under "Decisions this plan settles".

## Risks

- **Tauri API surface.** `on_new_window`, and builder options for accelerator
  keys and context menus, depend on the resolved Tauri and wry versions. The
  fallback is `with_webview` with `webview2-com` pinned to the exact version
  `wry` resolves. Confirm that `cargo tree -i webview2-com` shows a single
  version.
- **`eval` under CSP.** The close handshake relies on WebView2 `ExecuteScript`
  running despite `script-src 'self'`. If it were blocked, no acknowledgement
  would arrive, and the 3-second "no page answered" path would close the
  window and lose pending drafts.
  - The startup eval self-test (Phase 7) makes this visible in the log on
    every launch.
  - Phase 7 and Phase 8 verification require "eval self-test ok" and "page
    confirmed all edits saved".
  - Fallbacks: an initialization script, or a CSP hash for the fixed snippet.
- **Abrupt termination.** Task Manager kill, Ctrl+C in the development console,
  and operating-system shutdown or logoff bypass the handshake. An edit younger
  than the save delay can be lost; committed transactions are durable.
- **A hung page.** If the page's script thread is hung, no acknowledgement
  arrives and the window closes after 3 s. Its drafts are lost, but nothing
  could have saved them.
- **Release builds without the `custom-protocol` feature.** As in TTTB, plain
  `cargo build --release` leaves Tauri in its "dev" configuration. This is
  harmless because the window never uses Tauri's asset resolver or `devUrl`.
  The Phase 8 self-containment rehearsal (with `frontend\dist` renamed) is the
  proof.
- **The interim period (Phases 1–8).** Lars's daily app is the browser
  snapshot. If he also starts a desktop build pointed at the real location,
  two writers share one database with stale views, because single instance
  does not span the snapshot. The Phase 8 switch-over stops the snapshot
  first.
- **Junction tests need `mklink /J`.** It needs no elevation, but it is
  Windows-only. On other platforms those tests are compiled out; the
  application is Windows-only.
- **The default real path is defined in three places**: the desktop crate, the
  migration script and the install script. Each script carries a comment
  naming the Rust definition.
- **Plain-http images in notes stop rendering.** This is accepted by the
  brief.
- **Single-instance hand-off to an older running copy.** The install script
  refuses to run while the installed copy is running.
- **Relaunch during the shutdown tail.** The single-instance plugin releases
  its mutex on `RunEvent::Exit` before the shell drains requests and closes
  the database pool (up to 5 s). A relaunch can open the same SQLite file
  during that tail; SQLite cross-process locking protects committed data,
  and the old process admits no new API requests and has no UI. This overlap
  is accepted. If a strict guarantee is ever needed, the shell must hold a
  named mutex until `run()` returns.
- **WebView2 runtime.** It is present on Windows 11 and is not bundled. The
  README states the prerequisite.
- **The debug development window serves whatever `frontend/dist` holds.**
  `start.ps1` always rebuilds it unless `-SkipBuild` is given.

## Open Questions

None remain. All four questions from the first draft are answered and recorded
under Resolved questions (items 8–12). Review findings that change scope later
should be raised against this plan before the affected phase starts.
