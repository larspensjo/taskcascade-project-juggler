# Single Self-Contained Binary Packaging — Design

**Date:** 2026-07-11
**Status:** Approved (revised after review, see
`docs/2026-07-11-single-binary-packaging-review.md`)

## Summary

Package TaskCascade as one self-contained application file per platform
(Windows and Linux). The packaged binary embeds the built React frontend,
stores data in a cross-platform user directory, picks a free loopback port
dynamically, and opens the user's default browser automatically. Delivery is
"copy one file to a friend" — no installers, no CI, no bundled sidecar files.

The development workflow (`scripts/start.ps1` with the Vite dev server) keeps
working unchanged apart from one added environment variable and the renamed
binary. Ordinary backend development (`cargo build`, `cargo test`,
`cargo clippy`) keeps working from a clean checkout with no frontend build —
frontend embedding is opt-in via a Cargo feature used only by the packaging
scripts.

## Goals and Non-Goals

Goals:

- One application file per platform, with no bundled sidecar files and no
  application runtime to install; running it from any directory works.
  (SQLite is already compiled in via `libsqlite3-sys`. Platform system
  libraries remain: the Linux `x86_64-unknown-linux-gnu` build requires a
  glibc at least as new as the build machine's, and the Windows MSVC build
  dynamically links the ubiquitous VC runtime. An installed browser is
  likewise assumed. Packaging verification inspects the actual linkage — see
  Testing.)
- Automatic browser launch at the served URL.
- Dynamic port selection matching today's `start.ps1` behaviour: prefer 8080,
  scan upward to the first free port.
- Linux support (x86_64, standard glibc target).
- Backend verification gates (`cargo test`,
  `cargo clippy --all-targets -- -D warnings`) still pass on a clean checkout
  without Node dependencies or a frontend build.

Non-goals (out of scope):

- CI pipelines, GitHub Releases, installers, code signing, auto-update.
- macOS builds (nothing prevents them later; simply not produced now).
- Fully static builds (musl Linux, static CRT Windows). If literal runtime
  independence ever becomes a requirement, that is a follow-up design.
- Multi-user or network access — the server stays bound to `127.0.0.1`.

## Backend Changes

### 1. Embed the frontend behind an `embed-frontend` Cargo feature

Add an `embed-frontend` feature to `backend/Cargo.toml`. Only packaging
builds enable it; default builds are untouched by the embedding machinery.

**With the feature enabled** (packaging): a `rust-embed` asset struct replaces
the disk-based fallback:

```rust
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../frontend/dist"]
struct FrontendAssets;
```

The `$CARGO_MANIFEST_DIR` interpolation makes the path explicit and
independent of compiler or runtime working directories (plain relative paths
have different resolution rules in `rust-embed` debug builds). Because the
feature is only enabled for `--release` packaging builds, assets are always
truly compiled into the executable; the crate's debug disk-reading mode is
never relied upon.

**Without the feature** (default; development): the fallback stays
`ServeDir::new("../frontend/dist")` exactly as today, resolved at runtime
from the working directory that `start.ps1` already sets. No compile-time
dependency on `frontend/dist` exists, so `cargo test` and
`cargo clippy --all-targets` work on a clean checkout.

`rust-embed` and `mime_guess` become optional dependencies tied to the
feature.

### 2. SPA fallback that does not swallow API errors

The current router mixes `/api` routes and the static fallback at one level;
`ServeDir` happens to return 404 for unknown `/api` paths. The embedded
fallback must preserve that: an API typo must never yield `200 text/html`.

- Nest all API routes in their own router under `/api`, whose fallback
  returns a plain `404` for any unmatched `/api/...` path or method.
- The site fallback (embedded or `ServeDir`) applies only outside `/api`.
  With `embed-frontend`: `GET`/`HEAD` requests are served the matching
  embedded asset, or `index.html` when no asset matches (single-page-app
  routing). Content types come from `mime_guess`. Non-`GET`/`HEAD` requests
  to non-routes return `405`/`404`, never the SPA document.

### 3. Dynamic port selection with strict explicit-port semantics

Port handling moves into the backend, as a small testable function used by
`main.rs`:

- **`TASKCASCADE_PORT` set:** bind exactly `127.0.0.1:{port}`; if the bind
  fails, exit with a clear error. No scanning. Supervisors that assign the
  port (`start.ps1`'s health check, Vite's `/api` proxy target) depend on the
  backend using precisely the assigned value; silently drifting to another
  port would strand them or, worse, leave them talking to whatever process
  took the original port.
- **`TASKCASCADE_PORT` unset** (the packaged binary's normal case): start at
  8080 and bind the first free port scanning upward (bounded at 65535, clear
  error if nothing is free). Binding directly — rather than test-then-close —
  avoids the race where a port is taken between check and use.

Print the resulting URL, exactly as today. `start.ps1` keeps its own
pre-scan and continues to pass an explicit port; its contract is now
guaranteed rather than assumed.

### 4. Automatic browser launch

After the listener is bound (the OS queues connections from that moment, so
there is no race with the browser), open `http://127.0.0.1:{port}` using the
`webbrowser` crate, which handles both Windows and Linux default browsers.

Suppression: if the environment variable `TASKCASCADE_NO_BROWSER` is set to
any non-empty value, skip the launch. `start.ps1` sets it, because in
development the URL the user wants is the Vite dev server, not the backend.
A browser-launch failure is non-fatal: log the URL and continue serving.

### 5. Cross-platform data directory

Replace the `USERPROFILE`-based lookup in `main.rs` with a pure
path-selection function (unit-testable with injected candidate directories)
resolving in this order:

1. `TASKCASCADE_DATA_DIR` environment variable, if set (unchanged).
2. The platform documents directory (`dirs::document_dir()`) +
   `TaskCascade`. On Windows this is `Documents/TaskCascade` via the Known
   Folders API (also handling relocated Documents folders). On Linux this is
   the configured `XDG_DOCUMENTS_DIR` — usually `~/Documents` on desktop
   distributions, but `dirs::document_dir()` returns `None` when no
   user-directory configuration exists; it does not synthesize `~/Documents`.
3. The platform data directory (`dirs::data_dir()`) + `TaskCascade`
   (`~/.local/share/TaskCascade` on Linux) when no documents directory is
   configured.
4. Last resort (both lookups fail): relative `.local`, as today.

For existing Windows users with a default Documents location, the resolved
path is identical to today's — no data migration.

### 6. Remove the CORS layer

`CorsLayer::permissive()` is unnecessary even in development: the frontend
sends relative `/api` requests (`frontend/src/api.ts`) and Vite proxies them
server-side to the backend (`frontend/vite.config.ts`), so the browser never
makes a cross-origin request. In a shipped binary a permissive policy would
let any website the user visits read and write their tasks. Remove the layer
entirely — no debug/release split needed.

### 7. Binary name

Set `[[bin]] name = "taskcascade"` in `backend/Cargo.toml` so the deliverable
is `taskcascade.exe` / `taskcascade` rather than `taskcascade-backend`.
`start.ps1` references the binary path and is updated to match.

New dependencies: `webbrowser`, `dirs`; `rust-embed` and `mime_guess` as
optional dependencies behind `embed-frontend`.

## Packaging Scripts

### `scripts/package.ps1` (Windows)

1. `npm install` + `npm run build` in `frontend/`.
2. `cargo test --features embed-frontend` in `backend/` (runs the
   feature-gated embedded-asset tests against the freshly built assets).
3. `cargo build --release --features embed-frontend` in `backend/`.
4. Copy `backend/target/release/taskcascade.exe` to `dist/taskcascade.exe`.

### `scripts/package.sh` (Linux / WSL)

Same steps in POSIX shell, producing `dist/taskcascade`. The intended
workflow for producing the Linux binary from this Windows machine is running
the script inside WSL; the standard `x86_64-unknown-linux-gnu` target is
sufficient for current mainstream distributions (glibc no older than the
build machine's — a reason to build in a not-too-recent WSL distribution).

`dist/` is added to `.gitignore`.

## `scripts/start.ps1` Adjustments

- Set `TASKCASCADE_NO_BROWSER=1` before starting the backend.
- Point at the renamed binary (`target/debug/taskcascade.exe`).

Everything else — build order, port scanning, health-check waiting, process
supervision — stays as is. (The default backend build has no compile-time
frontend dependency, so no build-order change is needed.)

## Documentation Reconciliation

- **`README.md`:** add a "Package for distribution" section describing
  `package.ps1` / `package.sh` (WSL note included) and what the produced
  binary does on first run (creates the data directory, picks a port, opens
  the browser). Mention `TASKCASCADE_PORT` (exact-bind semantics),
  `TASKCASCADE_DATA_DIR`, and `TASKCASCADE_NO_BROWSER`, and state the
  platform expectations (Windows with the common VC runtime; Linux with a
  compatible glibc and a desktop browser).
- **`docs/DecisionLog.md`:** dated entry (2026-07-11) recording: single-binary
  distribution chosen over Tauri and Docker; frontend embedded via
  `rust-embed` behind an `embed-frontend` feature so plain backend builds
  need no frontend assets; explicit `TASKCASCADE_PORT` binds exactly or
  fails, scanning happens only from the 8080 default; Linux data directory
  follows configured XDG Documents with XDG data dir fallback; CORS layer
  removed as unnecessary (Vite proxies `/api` server-side); no CI/installers
  for now.
- **`docs/Design.HighLevel.md`:** update the serving/architecture description
  if it mentions `ServeDir`/`frontend/dist` serving or CORS.

## Testing

Existing backend tests exercise `router()` over `/api` routes and are
unaffected; they must still pass — including from a clean checkout with
neither `frontend/node_modules` nor `frontend/dist` present (this is the
regression gate for the no-frontend-prerequisite goal).

New automated coverage:

- **Port selection (unit tests on the bind function):** occupy an ephemeral
  port, then (a) scanning without an explicit port skips it and binds the
  next one; (b) an explicit `TASKCASCADE_PORT` pointing at the occupied port
  produces an error, not a silent rebind.
- **Data-directory selection (unit tests on the pure function):** explicit
  override wins; documents directory used when available; data directory
  used when documents is unavailable; `.local` fallback when both are.
- **API namespace (default features):** unknown `/api/...` path returns 404;
  `POST` to a non-route returns a non-HTML error (405/404), not the SPA
  document.
- **SPA fallback (gated behind `embed-frontend`, run by the packaging
  scripts):** `GET /` and an unknown client route both return the
  `index.html` content; a known asset returns its content type.

Manual verification (release build):

1. Run `scripts/package.ps1`; copy `dist/taskcascade.exe` to a directory
   outside the repository and run it from there — UI must load fully (fonts,
   icons, styles), confirming no filesystem dependency on the repo.
2. Browser opens automatically at the printed URL.
3. Start a second instance while the first runs — it must pick the next port
   (8081) and open a second tab.
4. Verify data lands in `Documents/TaskCascade` and survives restart.
5. Inspect artifact linkage: `dumpbin /dependents dist/taskcascade.exe` on
   Windows, `ldd dist/taskcascade` on Linux — confirm only expected system
   libraries appear and record the observed glibc requirement.
6. On Linux (WSL or a real machine): run `scripts/package.sh`, run the
   binary, verify data directory resolution and that the UI loads. (Browser
   launch from inside WSL may not work — acceptable; the printed URL covers
   it.)
7. `scripts/start.ps1` still works for development end to end — bootstrap and
   mutations function through the Vite proxy, confirming nothing needed the
   removed CORS layer — and no backend browser tab opens automatically.

## Out of Scope

- CI, GitHub Releases, installers, winget/apt packaging, code signing.
- macOS support.
- musl static Linux builds and static-CRT Windows builds.
- Tray icon, single-instance enforcement, auto-update.
