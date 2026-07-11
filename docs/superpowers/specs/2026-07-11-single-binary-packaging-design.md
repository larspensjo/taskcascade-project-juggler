# Single Self-Contained Binary Packaging — Design

**Date:** 2026-07-11
**Status:** Approved

## Summary

Package TaskCascade as one self-contained executable per platform (Windows and
Linux). The binary embeds the built React frontend, stores data in a
cross-platform user directory, picks a free loopback port dynamically, and
opens the user's default browser automatically. Delivery is "copy one file to
a friend" — no installers, no CI, no runtime dependencies (SQLite is already
compiled in via `libsqlite3-sys`).

The development workflow (`scripts/start.ps1` with the Vite dev server) keeps
working unchanged, apart from two small script adjustments noted below.

## Goals and Non-Goals

Goals:

- One executable per platform; running it from any directory works.
- Automatic browser launch at the served URL.
- Dynamic port selection matching today's `start.ps1` behaviour: prefer 8080,
  scan upward to the first free port.
- Linux support (x86_64, standard glibc target).

Non-goals (out of scope):

- CI pipelines, GitHub Releases, installers, code signing, auto-update.
- macOS builds (nothing prevents them later; simply not produced now).
- Multi-user or network access — the server stays bound to `127.0.0.1`.

## Backend Changes

### 1. Embed the frontend (`rust-embed`)

Replace the `ServeDir::new("../frontend/dist")` fallback in `router()`
(`backend/src/app.rs`) with a fallback handler backed by the `rust-embed`
crate:

```rust
#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct FrontendAssets;
```

- The `folder` path resolves relative to the backend crate root, so it is
  independent of the working directory.
- In **release** builds the files are compiled into the executable.
- In **debug** builds `rust-embed` reads the files from disk on every request,
  so frontend rebuilds during development are picked up without recompiling
  the backend.
- The fallback handler: serve the asset matching the request path; for the
  root path (and any path not matching an asset) serve `index.html` — the
  frontend is a single-page app. Determine `Content-Type` with the
  `mime_guess` crate.

**Build-order constraint:** `frontend/dist` must exist when the backend is
compiled (the `rust-embed` macro requires the folder at compile time).
`start.ps1` currently builds the backend *before* the frontend; swap those two
steps so a clean checkout builds without manual intervention.

### 2. Dynamic port selection

Move the port scan from `start.ps1` into `main.rs`: starting at
`TASKCASCADE_PORT` (default `8080`), attempt to bind
`127.0.0.1:{port}` and increment on failure until a bind succeeds (bounded at
65535, with a clear error if nothing is free). Binding directly — rather than
test-then-close as the script does — avoids the race where a port is taken
between the check and the real bind. Print the resulting URL, exactly as
today.

`start.ps1` keeps its own scan (it also needs a Vite port); the backend scan
simply makes the standalone binary self-sufficient. When `start.ps1` passes an
explicit `TASKCASCADE_PORT` that turns out busy, the backend now scans upward
from it instead of failing — harmless, since the script pre-checks
availability anyway.

### 3. Automatic browser launch

After the listener is bound (the OS queues connections from that moment, so
there is no race with the browser), open `http://127.0.0.1:{port}` using the
`webbrowser` crate, which handles both Windows and Linux default browsers.

Suppression: if the environment variable `TASKCASCADE_NO_BROWSER` is set to
any non-empty value, skip the launch. `start.ps1` sets it, because in
development the URL the user wants is the Vite dev server, not the backend.
A browser-launch failure is non-fatal: log the URL and continue serving.

### 4. Cross-platform data directory

Replace the `USERPROFILE`-based lookup in `main.rs` with the `dirs` crate:

1. `TASKCASCADE_DATA_DIR` environment variable, if set (unchanged).
2. `dirs::document_dir()` + `TaskCascade` — resolves to `Documents/TaskCascade`
   on Windows (now via the Known Folders API, which also handles relocated
   Documents folders) and `~/Documents/TaskCascade` on Linux, mirroring the
   Windows layout as decided during design.
3. Fallback when no documents directory exists (e.g. minimal Linux setups
   without XDG user dirs): `dirs::data_dir()` + `TaskCascade`
   (`~/.local/share/TaskCascade` on Linux).
4. Last resort (both lookups fail): relative `.local`, as today.

For existing Windows users with a default Documents location, the resolved
path is identical to today's — no data migration.

### 5. CORS tightened to development builds

`CorsLayer::permissive()` exists only so the Vite dev server (a different
origin) can call the API. In the packaged binary the frontend is same-origin,
and a permissive CORS policy would let any website the user visits read and
write their tasks from the browser. Apply the CORS layer only in debug builds
(`#[cfg(debug_assertions)]`); release builds add no CORS layer.

### 6. Binary name

Set `[[bin]] name = "taskcascade"` in `backend/Cargo.toml` so the deliverable
is `taskcascade.exe` / `taskcascade` rather than `taskcascade-backend`.
`start.ps1` references the binary path and is updated to match.

New dependencies: `rust-embed`, `mime_guess`, `webbrowser`, `dirs`.

## Packaging Scripts

### `scripts/package.ps1` (Windows)

1. `npm install` + `npm run build` in `frontend/`.
2. `cargo build --release` in `backend/`.
3. Copy `backend/target/release/taskcascade.exe` to `dist/taskcascade.exe`.

### `scripts/package.sh` (Linux / WSL)

Same three steps in POSIX shell, producing `dist/taskcascade`. The intended
workflow for producing the Linux binary from this Windows machine is running
the script inside WSL; the standard `x86_64-unknown-linux-gnu` target is
sufficient for current mainstream distributions. (A fully static musl build is
a possible later upgrade, not part of this design.)

`dist/` is added to `.gitignore`.

## `scripts/start.ps1` Adjustments

- Build the frontend before the backend (build-order constraint above).
- Set `TASKCASCADE_NO_BROWSER=1` before starting the backend.
- Point at the renamed binary (`target/debug/taskcascade.exe`).

Everything else — port scanning, health-check waiting, process supervision —
stays as is.

## Documentation Reconciliation

- **`README.md`:** add a "Package for distribution" section describing
  `package.ps1` / `package.sh` (WSL note included) and what the produced
  binary does on first run (creates `Documents/TaskCascade`, picks a port,
  opens the browser). Mention `TASKCASCADE_PORT`, `TASKCASCADE_DATA_DIR`, and
  `TASKCASCADE_NO_BROWSER`.
- **`docs/DecisionLog.md`:** dated entry (2026-07-11) recording: single-binary
  distribution chosen over Tauri and Docker; frontend embedded via
  `rust-embed`; Linux data directory mirrors Windows
  (`~/Documents/TaskCascade`, XDG data dir as fallback); CORS restricted to
  debug builds; no CI/installers for now.
- **`docs/Design.HighLevel.md`:** update the serving/architecture description
  if it mentions `ServeDir`/`frontend/dist` serving.

## Testing

Existing backend tests exercise `router()` over `/api` routes and are
unaffected by the fallback change; they must still pass.

New automated coverage:

- A router-level test asserting the fallback serves `index.html` content for
  `/` and an unknown path (single-page-app behaviour), using the debug-mode
  disk-backed assets. Note: compiling the backend (and therefore running any
  backend test) requires `frontend/dist` to exist — a consequence of the
  build-order constraint above, and why `start.ps1` builds the frontend first.

Manual verification (release build):

1. Run `scripts/package.ps1`; copy `dist/taskcascade.exe` to a directory
   outside the repository and run it from there — UI must load fully (fonts,
   icons, styles), confirming no filesystem dependency on the repo.
2. Browser opens automatically at the printed URL.
3. Start a second instance while the first runs — it must pick the next port
   (8081) and open a second tab.
4. Verify data lands in `Documents/TaskCascade` and survives restart.
5. On Linux (WSL or a real machine): run `scripts/package.sh`, run the binary,
   verify data directory resolution and that the UI loads. (Browser launch
   from inside WSL may not work — acceptable; the printed URL covers it.)
6. `scripts/start.ps1` still works for development: Vite URL opens nothing
   automatically from the backend, live frontend editing still functions.

## Out of Scope

- CI, GitHub Releases, installers, winget/apt packaging, code signing.
- macOS support.
- musl static Linux builds.
- Tray icon, single-instance enforcement, auto-update.
