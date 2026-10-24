# Agent Access to TaskCascade (CLI Subcommands) — Implementation Plan

**Parked (DecisionLog 2026-10-24, desktop application):** TaskCascade is now a desktop app that opens no port, so this plan's loopback client cannot reach it; the plan is parked, and any CLI would need a separate console binary because the desktop executable is a GUI-subsystem program.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan phase-by-phase. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** AI coding agents working in any repository on this machine can search
TaskCascade tasks, read one task in full, list a project's stack, and add tasks
— without the web UI — by running `taskcascade` subcommands that talk to the
user's already-running TaskCascade instance over the loopback API.

**Architecture:** The existing single binary gains subcommands. `taskcascade`
with no subcommand runs the server exactly as today; `taskcascade
projects|search|list|show|add` run as a short-lived HTTP client and exit. The
client path never opens SQLite — the server stays the sole enforcer of every
invariant. The server announces itself at startup in a well-known registry
directory (one file per running server: port, pid, resolved data directory,
refresh timestamp) and keeps that file fresh with a heartbeat; the client
discovers the live instance from that registry, verifies it with
`GET /api/health`, and refuses to act against an instance that is not serving
the expected data directory (the `scripts/start.ps1` development instance runs
on a scratch database in `.local/data`). `--port` bypasses the registry
entirely and probes that port directly, so deliberate targeting still works
when registration failed. Read endpoints the CLI needs — a lightweight project
list, a scoped/limited search, a scoped task list, and a single-task fetch —
are added server-side, because the only existing way to see projects is
`GET /api/bootstrap`, which returns the whole database. The only mutation the
CLI can perform is creating a task through the existing `POST /api/tasks`.

**Tech Stack:** Rust (axum, sqlx/SQLite) plus three new backend dependencies —
`clap` (subcommand parsing), `ureq` with no TLS features (blocking loopback
HTTP client), `dirs` (already specified by the approved packaging design) — and
`tempfile` as a dev-dependency. React 19 + TypeScript for the single frontend
change (refetch on window focus). No Cargo workspace: everything lives in the
one `backend/` package.

**Design sources:** the two packaging decisions this plan adopts — one
distributed file named `taskcascade`, and the data-directory resolution order —
originated in the approved packaging design, whose document now exists only in
git history
(`git show HEAD:docs/superpowers/specs/2026-07-11-single-binary-packaging-design.md`);
this plan, and after implementation `docs/AgentAccess.md`, are the working
source of truth for those two items. The rest of that design (frontend
embedding, port scanning, browser launch, CORS removal) remains future work.
`docs/DecisionLog.md` (local-first loopback-only, position-is-priority,
archive/trash semantics) applies as ever. The durable contract produced by this
work lands in `docs/AgentAccess.md` and a new `docs/DecisionLog.md` entry.

---

## Global Constraints

- Run backend commands from `backend/`, frontend commands from `frontend/`
  (per `Agents.md`).
- Backend gate: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt`. Frontend gate: `npm run check`, `npm run fmt`.
- The CLI code path must never open the SQLite database, run a migration, or
  import `sqlx`. All reads and writes go through the loopback HTTP API.
- Pure derivation (LIKE-pattern escaping, search excerpts, project-name
  matching, path normalization, instance selection and reaping, output
  formatting) lives in `domain` or in pure `cli` helpers with unit tests; HTTP
  and SQL stay in the app/server layer (`Agents.md` → Architecture).
- No new mutations: agents can create tasks and nothing else. Position is
  still the only priority; created tasks go to the top of the stack, like the
  UI.
- Loopback only. The client dials `127.0.0.1` and never any other host; the
  HTTP client is compiled without a TLS backend so `https://` is not even
  reachable.
- Every automated test that spawns a server **must** set
  `TASKCASCADE_INSTANCE_DIR` and `TASKCASCADE_DATA_DIR` to temporary
  directories. A test must never write to the developer's real registry or
  database.
- Data-directory strings are always produced by one shared normalizer
  (`data_dir::canonical_display`) — on the server before they reach health and
  the registry, and on the client before they are compared. Nothing compares
  raw `Path` values.
- Commit messages follow repo style: short imperative sentence, no `feat:`
  prefixes.
- New flags and thresholds ship with defaults that exercise the new code path
  (the CLI always sends a `limit`; the registry is always written and
  refreshed).

## Decisions this plan settles

Rationale is recorded here and summarised in `docs/AgentAccess.md` and the
decision-log entry.

1. **Output format: human-readable text by default, `--json` for machines.**
   Agents read line-oriented text well and it costs far fewer tokens than
   pretty JSON; scripts and any future MCP layer get exact API shapes via
   `--json`. Both formats are stable and covered by unit tests.
2. **Bodies: summaries by default, `show`/`--full` for whole bodies.** `search`
   and `list` print one line per task (id, state, project, title) plus, for
   `search`, one indented excerpt line around the match. `show <id>` and
   `--full` print the complete description and scratchpad in a delimited
   block. Because raw Markdown can contain any line, block text output is for
   humans; the skill teaches "search, then show", and `--json` whenever bodies
   must be parsed.
3. **Instance discovery is a registry directory, one small announcement file
   per running server** (approved), rather than a single fixed file. A single
   file is overwritten by whichever server started last, so a development
   instance started after the real one would mask it and the CLI would refuse
   every command while the user's real instance was running.
4. **Deliberate targeting is explicit and registry-independent.** `--port <n>`
   probes that port directly — no registry lookup, no data-directory check —
   so it still works when registration failed or the record is stale. An
   explicitly set `TASKCASCADE_DATA_DIR` shifts what "expected" means and is
   the path used by tests and by development work on the CLI itself. With
   neither, a mismatch is a refusal, not a warning.
5. **Assets live in `agent-access/` (verbatim, copyable) and the durable
   contract in `docs/AgentAccess.md`** — `Agents.md` puts lasting contracts
   under `docs/`, while a skill directory needs to be copied byte-for-byte to
   `~/.claude/skills/`, which reads badly inside `docs/`.
6. **Version skew is detected by capability, not semver.** The running server
   may be an older build than the CLI binary the agent invokes. If health
   lacks the `name`/`dataDir` fields, or a required endpoint 404s, the CLI
   says "restart TaskCascade — the running instance is an older build" and
   exits 2. No version bump accompanies this work; the repository has no
   version policy.
7. **This plan installs the binary itself.** The packaging design's
   `package.ps1`/`package.sh` do not exist yet, so Phase 7 builds a release
   binary and copies it to a PATH directory as part of
   `scripts/install-agent-access.ps1`. This plumbing is explicitly provisional
   and is expected to be replaced when the packaging design is implemented.

## Command surface (the contract implemented across Phases 2–5)

```
taskcascade                                   # runs the server/app (unchanged)
taskcascade projects [--json]
taskcascade search <query> [--project <name>] [--state active|archived|all]
                          [--limit <n>] [--full] [--json]
taskcascade list --project <name> [--state active|archived|all]
                 [--limit <n>] [--full] [--json]
taskcascade show <id> [--json]
taskcascade add --project <name> --title <title>
                [--description <text> | --description-file <path|->] [--json]
```

Global options (declared with clap's `global = true`, so they are accepted
before or after the subcommand): `--port <n>`, `--json`, `--help`.
Defaults: `search --limit 20`, `list --limit 50`, `--state all` for `search`,
`--state active` for `list`.

Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | Success (also `--help`/`--version`, which print to stdout) |
| 1 | Usage or validation error — bad flags (clap parse failure), unknown project, unknown task id (a 404 from `GET /api/tasks/{id}`), empty title, HTTP 400 |
| 2 | No usable instance — not running, stale registry, wrong data directory, older build (health lacks the capability fields, or a collection endpoint — `GET /api/projects`, `GET /api/search`, `GET /api/tasks` — answers 404), `--port` unanswered |
| 3 | Server or transport failure — HTTP 5xx, unreadable or unparseable response |

Clap's own exit code (2) must never escape: the dispatcher parses with
`try_parse_from` and maps parse failures to 1, so exit code 2 unambiguously
means "no usable instance".

---

### Phase 0: Commit the documentation reorganization

The working tree carries an uncommitted docs reorganization
(`docs/superpowers/**` deleted; this plan and its review now live under
`docs/Plans/`). Every later phase ends with `git add -A && git commit`, which
would silently sweep that reorganization into an unrelated code commit.

- [ ] **Step 1: Commit the docs move on its own, before any implementation**

```powershell
git add -A && git commit -m "Move plan documents to docs/Plans"
```

---

### Phase 1: Server foundations — binary name, data directory, instance registry, health facts

Server-side only; no CLI yet. This phase adopts exactly two items that
originated in the approved packaging design — the binary name `taskcascade`
and the data-directory resolution order — because discovery depends on both.
That design document is no longer in the working tree, so the specifications
in this phase stand on their own; the rest of that design (frontend embedding,
port scanning, browser launch, CORS removal) is untouched.

**Files:**

- Modify: `backend/Cargo.toml` (bin name, `dirs`, tokio `signal` feature,
  `tempfile` dev-dependency), `backend/src/main.rs`, `backend/src/lib.rs`,
  `backend/src/app.rs` (AppState shape, health handler), `scripts/start.ps1`
  (binary path **and** child-scoped environment).
- Create: `backend/src/data_dir.rs`, `backend/src/instance.rs`,
  `backend/src/server.rs`, `backend/tests/health.rs`.

**Interfaces produced (consumed by Phases 2–5):**

```rust
// data_dir.rs — pure resolution and normalization, plus thin env/dirs wrappers
pub fn resolve(over: Option<PathBuf>, documents: Option<PathBuf>, data: Option<PathBuf>) -> PathBuf;
pub fn from_env() -> PathBuf;             // TASKCASCADE_DATA_DIR → documents → data → ./.local
pub fn default_location() -> PathBuf;     // same, ignoring TASKCASCADE_DATA_DIR
pub fn canonical_display(path: &Path) -> String;  // the ONE normalizer (see Step 4)
pub fn same_location(a: &str, b: &str) -> bool;   // compares canonical_display outputs

// instance.rs
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRecord {
    pub port: u16, pub pid: u32, pub data_dir: String,   // already canonical_display'd
    pub started_at: String, pub refreshed_at: String, pub version: String,
}
pub fn registry_dir() -> PathBuf;            // TASKCASCADE_INSTANCE_DIR, else the pure fn below
pub fn registry_dir_from(over: Option<PathBuf>, local_data: Option<PathBuf>,
                         home: Option<PathBuf>, temp: PathBuf) -> PathBuf;
pub fn write(record: &InstanceRecord) -> std::io::Result<PathBuf>;  // replaces any existing file
pub fn remove(port: u16);                                           // best effort
pub fn read_all() -> Vec<(PathBuf, InstanceRecord)>;                // skips unreadable/garbage
```

`GET /api/health` gains `name`, `pid`, and `dataDir`:
`{"name":"taskcascade","version":"0.1.0","pid":1234,"dataDir":"C:\\Users\\…\\Documents\\TaskCascade"}`
(`dataDir` is `null` when the server was built without one, as in in-process
router tests). The value is the same `canonical_display` string that goes into
the registry record — computed once at startup and carried in `AppState`.

- [ ] **Step 1: Write the failing tests**

Create `backend/tests/health.rs` asserting that `GET /api/health` returns
`name == "taskcascade"`, a non-empty `version`, a numeric `pid`, and a
present-but-null `dataDir` for `AppState::connect("sqlite::memory:")`, and the
exact `canonical_display` string for
`AppState::connect(...).with_data_dir(<temp dir>)`. Reuse the `send()` helper
pattern from `backend/tests/trash.rs`.

Unit tests in `data_dir.rs`:

- the four resolution branches (override wins; documents when present; data
  dir when documents is absent; `.local` when both are absent);
- `canonical_display` strips a Windows `\\?\` verbatim prefix, removes a
  trailing separator, and resolves `..` segments for an existing directory,
  and still returns an absolute-looking string for a directory that does not
  exist (fallback path);
- `same_location` on two `canonical_display` outputs: identical, differing
  case (equal on Windows, unequal elsewhere), and unrelated paths.

Unit tests in `instance.rs`:

- `registry_dir_from` precedence;
- a write/`read_all` round trip in a `tempfile::TempDir` that also contains a
  non-JSON file (skipped, not a panic);
- **`write` replaces an existing record for the same port** — write a record,
  then write a second one for the same port with a different pid and data
  directory, and assert `read_all` yields exactly one record carrying the
  second one's values (regression for the restart-on-the-same-port case).

- [ ] **Step 2: Run tests to verify they fail**

Run (from `backend/`): `cargo test`
Expected: FAIL — `health.rs` does not compile/pass (no `with_data_dir`, no
`name` field); `data_dir`/`instance` modules do not exist.

- [ ] **Step 3: Rename the binary and add dependencies**

In `backend/Cargo.toml` add `[[bin]] name = "taskcascade"`, `path = "src/main.rs"`.
Add `dirs = "6"` (client dependencies come in Phase 2), extend the tokio
features with `"signal"`, and add `tempfile = "3"` under `[dev-dependencies]`.
Update `scripts/start.ps1` line 78 to `target/debug/taskcascade.exe`.
(`.vscode/tasks.json` references only cargo commands and needs no change.)

- [ ] **Step 4: Extract data-directory resolution and normalization**

Create `backend/src/data_dir.rs`. `resolve` implements the approved order:
`TASKCASCADE_DATA_DIR` → `dirs::document_dir()/TaskCascade` →
`dirs::data_dir()/TaskCascade` → `PathBuf::from(".local")`. This order is
specified normatively here and, after Phase 7, in `docs/AgentAccess.md`; the
packaging design document that first recorded it survives only in git history.

`canonical_display` is the single normalizer used by **every** producer and
consumer of a data-directory string:

1. `std::fs::canonicalize(path)`; on success, on Windows strip a leading
   `\\?\` (and `\\?\UNC\` → `\\`) prefix.
2. On failure (directory missing or inaccessible), fall back to
   `std::path::absolute(path)` — or joining the current directory when that
   also fails — then lexically drop `.` segments and collapse `..`.
3. Strip any trailing path separator.
4. Return the resulting `String`.

`same_location(a, b)` compares two such strings, case-insensitively when
`cfg!(windows)`, case-sensitively otherwise. It must never be called on raw
user input — the three call sites (server startup, registry record, CLI
expectation) all pass `canonical_display` output.

- [ ] **Step 5: Add the instance registry**

Create `backend/src/instance.rs`. `registry_dir_from` precedence:
`TASKCASCADE_INSTANCE_DIR` → `dirs::data_local_dir()/TaskCascade/instances` →
`home/.taskcascade/instances` → `temp/TaskCascade/instances`.

`write` must replace an existing record for the same port, on Windows as well
as on Unix:

1. Create the registry directory if needed.
2. Serialize to `instance-<port>.json.tmp-<pid>` in the same directory and
   flush it.
3. `std::fs::rename` the temporary file over `instance-<port>.json`, so a
   reader never observes a half-written record.
4. If the rename fails (a reader holding the destination open without
   `FILE_SHARE_DELETE` can cause a sharing violation on Windows), remove the
   destination and retry the rename; if that also fails, write the destination
   directly, truncating.
5. On any failure, clean up the temporary file and return the error.

`read_all` ignores files it cannot read or parse. `remove(port)` is best
effort.

- [ ] **Step 6: Move the server startup into `server.rs`, announce, and heartbeat**

Move today's `main` body into `pub fn run() -> Result<(), Box<dyn Error>>` in
`backend/src/server.rs`, which builds its own `tokio::runtime::Runtime`
(`main.rs` stops being `#[tokio::main]` so Phase 2 can dispatch to a blocking
client without a runtime). Changes while moving:

- Resolve the data directory with `data_dir::from_env()`, `create_dir_all` it,
  then compute `data_dir::canonical_display` **once** and use that one string
  for `AppState::with_data_dir` and for the registry record.
- `TASKCASCADE_PORT` unset → `8080` as today; set → bind exactly that value
  (`0` therefore means "OS-assigned ephemeral port", which the integration
  tests in Phase 2 rely on). Take the actual port from
  `listener.local_addr()?.port()` for both the printed URL and the record.
- After binding and before serving, `instance::write(&record)` with
  `started_at == refreshed_at == now()`; log a warning to stderr and keep
  serving if the write fails (a registry problem must never stop the app —
  `--port` remains the escape hatch in that case).
- Spawn a heartbeat task that rewrites the record every 60 seconds with a new
  `refreshed_at`. The registry therefore self-heals if a record is deleted or
  clobbered while the server is alive; combined with the client's reaping
  grace period (Phase 2), a still-starting or briefly-unresponsive server can
  never be permanently un-discoverable.
- Serve with `.with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; })`
  and call `instance::remove(port)` after `serve` returns. Note in a comment
  that `scripts/start.ps1` force-kills the process tree, so the file often
  survives; stale records are the client's problem to detect (Phase 2), and
  the next start on the same port replaces the file.

`main.rs` shrinks to a dispatcher that calls `server::run()` (Phase 2 adds the
subcommand branch). Export the new modules from `lib.rs`.

- [ ] **Step 7: Teach `AppState` about the data directory and extend health**

Convert `AppState` from the tuple struct to a named struct holding
`pool: Arc<SqlitePool>` and `data_dir: Option<Arc<str>>`, with
`pub async fn connect(...)` (data dir `None`),
`pub fn with_data_dir(self, canonical: String) -> Self`, and a private
`fn pool(&self) -> &SqlitePool`. Replace every `state.0.as_ref()` /
`state.0.begin()` / `&state.0` with `state.pool()`. Extend the `Health` struct
with `name: &'static str` (`"taskcascade"`), `pid: u32`
(`std::process::id()`), and `data_dir: Option<String>`, serialized
`rename_all = "camelCase"`.

- [ ] **Step 8: Stop `scripts/start.ps1` leaking its environment**

`start.ps1` currently sets `$env:TASKCASCADE_DATA_DIR` and
`$env:TASKCASCADE_PORT` process-wide and never restores them
(`scripts/start.ps1:73-78`, `:101-104`). Because an explicitly set
`TASKCASCADE_DATA_DIR` is defined here as deliberate authorization to use that
location, a leaked value would make later commands **in the same PowerShell
session** silently accept the `.local/data` scratch database instead of
refusing it.

Capture the previous values (including "was not set") before assigning them,
and restore them in the existing `finally` block — removing the variables with
`Remove-Item Env:TASKCASCADE_DATA_DIR` when they were previously unset. Keep
the assignment itself (the child processes inherit it); only the session-level
leak is removed. Add a comment naming the reason so the restore is not
"cleaned up" later.

- [ ] **Step 9: Run tests and the gate**

Run (from `backend/`): `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: PASS, including the pre-existing `restore.rs`, `trash.rs`, and
`project_colors.rs` suites.

- [ ] **Step 10: Human verification (recommended)**

Run `scripts/start.ps1`. Confirm: the app still works end to end; a file
`instance-<port>.json` appears under `%LOCALAPPDATA%\TaskCascade\instances\`
naming the repo's `.local/data` directory and the backend pid; its
`refreshedAt` advances after a minute; and
`http://127.0.0.1:<port>/api/health` reports exactly the same `dataDir`
string. Stop with Ctrl+C and check two things: `$env:TASKCASCADE_DATA_DIR` is
gone from the session, and note whether the record file remains (it usually
will, because the script force-kills the process tree) — that is the staleness
case Phase 2 handles. Start the script again and confirm the record is
replaced, not duplicated.

- [ ] **Step 11: Commit**

```powershell
git add -A && git commit -m "Announce the running instance and report its data directory"
```

---

### Phase 2: Smallest end-to-end slice — `taskcascade projects`

Delivers the whole client skeleton (dispatch, transport seam, discovery,
output, exit codes) plus the one endpoint it needs, and the spawned-server
integration harness that later phases reuse.

**Files:**

- Modify: `backend/Cargo.toml` (`clap`, `ureq`), `backend/src/main.rs`,
  `backend/src/lib.rs`, `backend/src/app.rs` (router + `list_projects`).
- Create: `backend/src/cli/mod.rs`, `backend/src/cli/args.rs`,
  `backend/src/cli/dto.rs`, `backend/src/cli/transport.rs`,
  `backend/src/cli/discovery.rs`, `backend/src/cli/output.rs`,
  `backend/src/cli/commands.rs`, `backend/tests/agent_api.rs`,
  `backend/tests/cli.rs`.

**Interfaces produced:**

```rust
// args.rs — the complete parsed context, not just the subcommand
#[derive(Parser)]
pub struct Cli {
    #[arg(long, global = true)] pub port: Option<u16>,
    #[arg(long, global = true)] pub json: bool,
    #[command(subcommand)] pub command: Option<Command>,
}

// transport.rs — the substitutable seam; commands are generic over it
pub struct HttpResponse { pub status: u16, pub body: String }
pub enum TransportError { Connect(String), Timeout, Protocol(String) }
pub trait Transport {
    fn get(&self, path: &str, query: &[(&str, String)]) -> Result<HttpResponse, TransportError>;
    fn post_json(&self, path: &str, body: &str) -> Result<HttpResponse, TransportError>;
}
pub struct HttpTransport { /* base "http://127.0.0.1:<port>", 1500 ms timeouts */ }

// discovery.rs — IO (probe) separated from pure decision functions
pub enum Probe { Ok(HealthInfo), Outdated(String), Refused, Unreachable, NotTaskCascade }
pub struct Candidate { pub record: InstanceRecord, pub probe: Probe }
pub enum Selection { Use(Candidate), NoneRunning, WrongDataDir(Vec<Candidate>), Outdated(Candidate) }
pub fn select(candidates: Vec<Candidate>, expected: &str) -> Selection;              // pure
pub fn should_reap(record: &InstanceRecord, probe: &Probe, now: DateTime<Utc>) -> bool;  // pure
pub fn connect(port_override: Option<u16>) -> Result<HttpTransport, CliError>;

// commands.rs — every command is generic and writes into a sink
pub fn projects<T: Transport, W: Write>(t: &T, out: &mut W, format: Format) -> Result<(), CliError>;

// output.rs — pure, unit-tested for stability
pub enum Format { Text, Json }
pub fn projects_text(projects: &[ProjectDto]) -> String;
```

`GET /api/projects` returns the existing `Project` shape
(`id`, `name`, `color`, `createdAt`) ordered by name — no task bodies, so it is
cheap at the documented workload. Registered as
`.route("/api/projects", get(list_projects).post(create_project))`.

- [ ] **Step 1: Write the failing tests**

In `backend/tests/agent_api.rs` (new; carries the API tests for Phases 2–4):
`GET /api/projects` returns an array containing the seeded `Personal` project
with `id`/`name`/`color` and no task fields.

In `backend/tests/cli.rs` (new; the spawned-process harness):

- `spawn_server(data_dir, registry_dir) -> ServerGuard` runs
  `env!("CARGO_BIN_EXE_taskcascade")` with `TASKCASCADE_DATA_DIR`,
  `TASKCASCADE_INSTANCE_DIR`, and `TASKCASCADE_PORT=0`; polls the registry
  directory (up to ~10 s) for a record, then polls `/api/health`; kills the
  child on `Drop`.
- `run_cli(args, envs) -> std::process::Output` runs the same binary.
- Tests:
  - `projects_lists_the_running_instances_projects` (exit 0, stdout contains
    `Personal`);
  - `missing_instance_explains_how_to_start` (empty registry → exit 2, stderr
    contains "not running");
  - `stale_record_is_ignored_and_reaped` (hand-written record for a port
    nothing listens on, `refreshedAt` two hours old → exit 2 and the file is
    gone afterwards);
  - `fresh_unanswered_record_is_not_reaped` (same, but `refreshedAt` = now →
    exit 2 **and the file survives**, protecting a server that is still
    starting);
  - `refuses_instance_serving_another_data_directory` (server on data dir A,
    CLI run with `TASKCASCADE_DATA_DIR=B` → exit 2, stderr names both
    directories and mentions `--port`);
  - `port_flag_bypasses_the_registry` (server running with an **empty**
    registry directory — delete the record after startup — plus
    `--port <the port>` → exit 0);
  - `port_flag_on_a_dead_port_exits_two`;
  - `invalid_usage_exits_one` (`taskcascade nonsense` → exit 1, never 2; the
    missing-query case lives in Phase 3, where `search` exists);
  - `help_exits_zero` (`--help` prints usage on stdout, exit 0).

Unit tests (in-module): `discovery::select` for none-live, one match,
non-match, two matches (lowest port wins) and missing health capability fields
(`Outdated`); `discovery::should_reap` for refused+old (true), refused+fresh
(false), timeout at any age (false), unreachable at any age (false),
not-TaskCascade+old (true), healthy (false); `output::projects_text` exact
string.

- [ ] **Step 2: Run tests to verify they fail**

Run (from `backend/`): `cargo test`
Expected: FAIL — no `/api/projects` GET route, no `cli` module, the binary
rejects arguments.

- [ ] **Step 3: Add `GET /api/projects`**

Add `list_projects` next to `bootstrap` in `app.rs`, reusing the same
`SELECT id, name, color, created_at FROM projects ORDER BY name` query, and
chain `get(list_projects)` onto the existing `/api/projects` route.

- [ ] **Step 4: Add dependencies and the dispatcher**

`backend/Cargo.toml`: `clap = { version = "4", features = ["derive"] }` and
`ureq = { version = "3", default-features = false }`. Verify with
`cargo tree -i rustls` (and `-i native-tls`) that no TLS backend is linked; if
the `ureq` feature matrix makes an HTTP-only build awkward, fall back to
`reqwest = { version = "0.12", default-features = false, features = ["blocking"] }`
— same transport seam — and note the swap in the commit message.

`main.rs` parses fallibly, keeps the whole parsed context, and owns the
exit-code mapping:

```rust
use clap::Parser;
use taskcascade_backend::{cli, server};

fn main() -> std::process::ExitCode {
    let cli = match cli::Cli::try_parse_from(std::env::args_os()) {
        Ok(cli) => cli,
        Err(error) => {
            // Help and version are successful output on stdout; every other
            // parse failure is a usage error (exit 1), never clap's own 2,
            // which this CLI reserves for "no usable instance".
            let usage = !matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            );
            let _ = error.print();
            return if usage { std::process::ExitCode::from(1) } else { std::process::ExitCode::SUCCESS };
        }
    };
    match cli.command {
        None => server::run().map_or(std::process::ExitCode::from(3), |()| std::process::ExitCode::SUCCESS),
        Some(_) => cli::run(cli),
    }
}
```

`cli::run(cli)` receives the entire `Cli` — `--port` and `--json` included —
resolves `Format` from `cli.json`, and returns an `ExitCode` from
`CliError::exit_code()` (1 usage/validation, 2 no usable instance, 3
transport/server), printing the message to stderr. Success output goes to
stdout. `error.print()` already routes help to stdout and errors to stderr.

- [ ] **Step 5: Implement the transport seam**

`HttpTransport` wraps `ureq` against `http://127.0.0.1:<port>`, with connect
and read timeouts of 1500 ms, returning `HttpResponse { status, body }` for
any HTTP status (including 4xx/5xx — the command layer decides) and a
`TransportError` variant that distinguishes a refused connection from a
timeout (the reaping rule depends on it). Provide a `#[cfg(test)]
FakeTransport` returning canned `(status, body)` per path so command logic is
unit-testable in-process.

- [ ] **Step 6: Implement discovery**

`connect(port_override)`:

- **`Some(port)` — registry bypassed entirely.** Probe
  `http://127.0.0.1:<port>/api/health` directly. A valid TaskCascade health
  response is used as-is, with **no** data-directory check (naming the port is
  the deliberate authorization). A missing capability field yields `Outdated`;
  refused/timeout/foreign response yields exit 2 with the "nothing answering"
  message. This path works even when `instance::write` failed at server
  startup.
- **`None`.** `instance::read_all()`, probe each record's `/api/health`, build
  `Candidate`s, then:
  - Delete a record only when `should_reap` says so: the probe was `Refused`
    or `NotTaskCascade` **and** `refreshed_at` is older than 120 seconds (two
    missed heartbeats). `Timeout` and `Unreachable` never reap — a busy or
    still-starting server must not be erased — and a fresh record is never
    reaped, which closes the race where a CLI invoked during startup deletes
    the only announcement.
  - Call the pure `select(candidates, expected)`, where `expected` is
    `data_dir::canonical_display(&data_dir::from_env())` — the default
    location unless the caller's environment explicitly sets
    `TASKCASCADE_DATA_DIR`. Prefer live candidates whose health `dataDir`
    `same_location`s `expected`, lowest port first; a live candidate missing
    capability fields yields `Outdated`; live candidates that all mismatch
    yield `WrongDataDir`; nothing live yields `NoneRunning`.

Messages (asserted by tests, so keep them stable — one line each in the code):

```text
NoneRunning:   TaskCascade is not running. Start it (run taskcascade, or
               scripts/start.ps1 in the TaskCascade repo) and try again —
               agents: report this to the user instead of starting it.
WrongDataDir:  Refusing the TaskCascade instance on port <p>: it stores data in
               <found>, not <expected>. That is usually a development instance.
               Re-run with --port <p>, or set TASKCASCADE_DATA_DIR, to use it
               deliberately.
Outdated:      The TaskCascade instance on port <p> is an older build (version
               <v>) without the agent API. Restart TaskCascade and try again.
PortUnanswered: Nothing is answering as TaskCascade on port <p>. Check the port,
               or omit --port to use the registered instance.
```

- [ ] **Step 7: Implement `projects` and its output**

`ProjectDto`/`TaskDto` in `cli/dto.rs` mirror the API JSON (camelCase).
`projects_text` prints one project name per line, in the server's order.
`--json` prints the API array verbatim (pretty-printed, trailing newline).

- [ ] **Step 8: Run tests and the gate**

Run (from `backend/`): `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: PASS.

- [ ] **Step 9: Human verification (recommended)**

The binary is not on `PATH` until Phase 7; use its full path
(`backend\target\debug\taskcascade.exe`) or prepend that directory to
`$env:PATH` for the session.

With `scripts/start.ps1` running (a *development* instance), open a shell in
another repository and run `taskcascade projects` — it must **refuse** with the
data-directory message, naming `.local/data`, and the same command with
`--port <dev port>` must succeed. Then stop `start.ps1`, run the built binary
as a plain server (against your real `Documents/TaskCascade`), and repeat: it
must list your real projects. Finally stop everything and confirm the "not
running" message and exit code 2 (`$LASTEXITCODE`).

- [ ] **Step 10: Commit**

```powershell
git add -A && git commit -m "Add taskcascade projects subcommand over the loopback API"
```

---

### Phase 3: Scoped, limited, wildcard-safe search — `taskcascade search`

**Files:**

- Modify: `backend/src/domain.rs`, `backend/src/app.rs` (search handler),
  `backend/src/cli/args.rs`, `backend/src/cli/commands.rs`,
  `backend/src/cli/dto.rs` (project index), `backend/src/cli/output.rs`,
  `backend/tests/agent_api.rs`, `backend/tests/cli.rs`.

**Interfaces produced:**

```rust
// domain.rs — pure search derivation and project-name matching
pub fn like_pattern(query: &str) -> String;   // escapes \ % _ and wraps in %…%
pub fn excerpt(text: &str, needle: &str, max_chars: usize) -> Option<String>;
pub enum ProjectMatch { Found(usize), NotFound }
pub fn find_project_by_name(names: &[String], wanted: &str) -> ProjectMatch;

// cli/dto.rs — id → name for EVERY rendered row, scoped or not
pub struct ProjectIndex { /* id → name */ }
impl ProjectIndex {
    pub fn load<T: Transport>(t: &T) -> Result<Self, CliError>;   // one GET /api/projects
    pub fn name_for<'a>(&'a self, id: &'a str) -> &'a str;        // falls back to the id
    pub fn id_for(&self, name: &str) -> Result<&str, CliError>;   // find_project_by_name
}

// cli/output.rs — the delimited full-body block, reused by `show` in Phase 4
pub fn task_block(task: &TaskDto, projects: &ProjectIndex) -> String;

// app.rs — GET /api/search?q=&projectId=&state=&limit=
enum TaskState { Active, Archived, All }      // shared with Phase 4
```

`find_project_by_name` lives here (not in Phase 4) because `search --project`
already needs it; Phase 4 consumes it unchanged.

`GET /api/search` keeps returning a **JSON array of tasks** so
`frontend/src/api.ts` is unaffected. New optional parameters: `projectId`,
`state` (`active`|`archived`|`all`, default `all`), `limit` (>= 1; absent means
unlimited, which is what the UI sends). `q` was already required by
deserialization; it now must also be non-empty (400 otherwise) — the UI never
sends an empty query. SQL uses
`LIKE ? ESCAPE '\'` with `domain::like_pattern`, keeps `deleted_at IS NULL`,
and keeps the existing ordering.

- [ ] **Step 1: Write the failing tests**

`backend/tests/agent_api.rs`:

- `search_treats_underscore_and_percent_literally` — create tasks "a_b" and
  "axb"; `?q=a_b` returns only "a_b". (Regression for the unescaped LIKE
  pattern at `backend/src/app.rs:536`.)
- `search_limits_results` — five matches, `?q=…&limit=2` returns 2.
- `search_scopes_by_project_and_state` — matches in two projects, one
  archived; `projectId` and `state=active` each narrow correctly and
  `state=archived` returns the archived one.
- `search_rejects_empty_query_and_zero_limit` — 400 for both.
- `search_still_excludes_deleted_tasks` — guards the trash invariant.

`domain.rs` unit tests for `like_pattern` (escapes `%`, `_`, `\`; leaves plain
text alone), `excerpt` (matching line trimmed and ellipsised,
case-insensitive, `None` when the needle is only in the title), and
`find_project_by_name` (exact, differing case, surrounding whitespace,
unknown).

`backend/tests/cli.rs`: `search_finds_a_seeded_task` (seed by POSTing to the
running server, then `taskcascade search`);
`search_reports_truncation_when_more_matches_exist`;
`search_rows_name_the_project_without_a_project_flag` (two projects, unscoped
search, both project names appear in the output);
`search_without_a_query_exits_one` (moved from Phase 2 — only now that the
subcommand exists does the assertion test what its name says).

Unit tests over `FakeTransport` for the text and `--json` shapes, `--full`
blocks, and the truncation notice.

- [ ] **Step 2: Run tests to verify they fail**

Run (from `backend/`): `cargo test`
Expected: FAIL — wildcards are still interpreted, and the new
`limit`/`state`/`projectId` parameters are silently ignored by the current
handler (`serde_urlencoded` skips unknown fields), so the scoping assertions
see wrong result sets; no `search` subcommand.

- [ ] **Step 3: Implement the server side**

Add `like_pattern`, `excerpt`, and `find_project_by_name` to `domain.rs`.
Widen `SearchQuery` to
`{ q: String, project_id: Option<String>, state: Option<String>, limit: Option<i64> }`
with `#[serde(rename_all = "camelCase", deny_unknown_fields)]` — the rejection
is deliberate: a future CLI sending a parameter this server does not know
fails loudly instead of silently returning unscoped results, and it is safe
for the UI, which only ever sends `q` (`frontend/src/api.ts:45`). Validate
(`required(&q, "Search query")`, `limit >= 1`, known `state`), and build the
statement from one shared helper that Phase 4 reuses:

```sql
SELECT <task columns> FROM tasks
WHERE deleted_at IS NULL <state predicate> <project predicate>
  AND (title LIKE ?1 ESCAPE '\' OR description LIKE ?1 ESCAPE '\' OR scratchpad LIKE ?1 ESCAPE '\')
ORDER BY completed_at IS NOT NULL, position, completed_at DESC
LIMIT ?
```

(Bind each pattern positionally as today if repeating `?1` complicates sqlx.)

- [ ] **Step 4: Implement the `search` subcommand**

Flags: `<query>` (positional, required), `--project`, `--state`, `--limit`
(default 20), `--full`; `--json` and `--port` are global.

Every invocation loads `ProjectIndex` (one `GET /api/projects`) — not only
when `--project` is given — because every rendered row names its project. When
`--project` is present the index also resolves it to an id; on `NotFound` the
command exits 1 with
`Unknown project "<name>". Known projects: A, B, C.`

The client requests `limit + 1` and, when the extra row comes back, prints
after the results:
`… more matches exist; re-run with --limit <2×limit> to see more.`

Text line format (one per task):

```
<id>  <state>  [<project>]  <title>
    <excerpt around the match>
```

`--full` switches to the same delimited block `show` uses (Phase 4):

```
--- task <id> ---
state: active
project: Engine
title: …
created: …   modified: …
description:
<raw markdown>
scratchpad:
<raw markdown>
--- end <id> ---
```

- [ ] **Step 5: Run tests and the gate**

Run (from `backend/`): `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: PASS.

- [ ] **Step 6: Human verification (recommended)**

With the app running, search from the UI for a term containing `_` or `%` and
confirm the results are now literal matches (previously they behaved as
wildcards), and that ordinary searches are unchanged. Then run
`taskcascade search "<term>"` and `taskcascade search "<term>" --full --json`
from another repository (full binary path until Phase 7).

- [ ] **Step 7: Commit**

```powershell
git add -A && git commit -m "Search accepts scope and limit and treats wildcards literally"
```

---

### Phase 4: Task reads — `taskcascade list --project <name>` and `taskcascade show <id>`

**Files:**

- Modify: `backend/src/app.rs` (`GET /api/tasks`, `GET /api/tasks/{id}`),
  `backend/src/cli/args.rs`, `backend/src/cli/commands.rs`,
  `backend/src/cli/output.rs`, `backend/tests/agent_api.rs`,
  `backend/tests/cli.rs`.

**Interfaces produced:**

- `GET /api/tasks?projectId=&state=&limit=` returns a JSON array of tasks
  ordered by the state's natural order (`active` → `position ASC`, `archived`
  → `completed_at DESC`, `all` → the search ordering), sharing the Phase 3
  query helper. Default `state` is `active`. Deleted tasks are never returned.
- `GET /api/tasks/{id}` returns one task, or 404 when the id is unknown **or
  the task is deleted** (consistent with the trash decision: deleted tasks are
  invisible to every operation except undelete). This is the first
  fetch-single-task endpoint in the API; the UI does not use it.
- `show` reuses `output::task_block` (introduced in Phase 3 for `--full`)
  unchanged, so the two commands cannot drift apart.

- [ ] **Step 1: Write the failing tests**

`backend/tests/agent_api.rs`: `tasks_endpoint_returns_project_stack_in_order`
(three tasks, newest first because creation inserts at the top);
`tasks_endpoint_scopes_by_state`; `tasks_endpoint_applies_limit`;
`tasks_endpoint_excludes_deleted`; `task_by_id_returns_the_task`;
`task_by_id_is_404_for_unknown_and_deleted`.

`backend/tests/cli.rs`: `list_shows_the_project_stack_top_first`;
`list_header_reflects_the_requested_state` (`--state archived` must not say
"active tasks"); `list_rejects_an_unknown_project_and_names_the_valid_ones`
(exit 1, stderr lists project names); `show_prints_the_full_task`;
`show_rejects_an_unknown_id` (exit 1).

Unit tests over `FakeTransport` for the three list headers and for
`task_block` output stability.

- [ ] **Step 2: Run tests to verify they fail**

Run (from `backend/`): `cargo test`
Expected: FAIL — `GET /api/tasks` is not routed (the path exists for POST
only), `GET /api/tasks/{id}` does not exist, no `list`/`show` subcommands.

- [ ] **Step 3: Implement the endpoints**

Route `.route("/api/tasks", get(list_tasks).post(create_task))` and
`.route("/api/tasks/{id}", get(get_task).put(update_task))`. `get_task`
selects the full task-column list with `WHERE id = ? AND deleted_at IS NULL`
and maps "no row" to `ApiError::not_found()`.

- [ ] **Step 4: Implement `list` and `show`**

`list --project <name>` resolves the name through `ProjectIndex` and prints
the Phase 3 row format under a header that matches the requested state:

```
<Project> · <n> active tasks (top of stack first)
<Project> · <n> archived tasks (most recently completed first)
<Project> · <n> tasks (active first, then archive)
```

When `--limit` truncated the result, the header count reads `showing <n> of
more` and the same `… re-run with --limit …` notice is printed after the rows.

`show <id>` fetches `GET /api/tasks/{id}` and prints `task_block` (title,
project, state, created/modified/completed timestamps, description,
scratchpad). `--json` prints the task JSON. A 404 exits 1 with
`No task with id "<id>". Ids come from the search and list output.`
This is the one 404 that means "missing resource"; a 404 from a collection
endpoint still means "older build" and exits 2 (Decision 6) — keep the two
paths distinct in `CliError`.

- [ ] **Step 5: Run tests and the gate**

Run (from `backend/`): `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add -A && git commit -m "Read a project's stack and a single task from the command line"
```

---

### Phase 5: Task creation — `taskcascade add`

Uses the existing `POST /api/tasks`; no server change. Creation therefore keeps
the UI's semantics exactly, including insertion at the top of the stack.

**Files:**

- Modify: `backend/src/domain.rs` (`normalize_markdown`),
  `backend/src/cli/args.rs`, `backend/src/cli/commands.rs`,
  `backend/src/cli/output.rs`, `backend/tests/cli.rs`.

**Interfaces produced:**

```rust
pub fn normalize_markdown(input: &str) -> String;  // CRLF → LF, trim trailing blank lines
```

- [ ] **Step 1: Write the failing tests**

`domain.rs` unit tests for `normalize_markdown` (CRLF input, trailing newlines,
content otherwise byte-identical — fenced code blocks and indentation must
survive untouched).

`backend/tests/cli.rs`:

- `add_creates_a_task_at_the_top_of_the_stack` — add two tasks, then
  `list --project …` shows the newest first.
- `add_reads_multiline_markdown_from_stdin` — spawn the CLI with
  `--description-file -`, pipe a multi-line Markdown document containing a
  fenced code block, a list, and a `#` heading, then read it back with
  `show <id> --json` and assert byte equality after normalization.
- `add_reads_description_from_a_file`.
- `add_rejects_an_unknown_project_without_creating_it` — exit 1, and
  `taskcascade projects` afterwards is unchanged (never auto-create).
- `add_rejects_an_empty_title` — exit 1.

Unit tests over `FakeTransport` for the success line and `--json` output, and
for the mutual exclusion of `--description` and `--description-file`.

- [ ] **Step 2: Run tests to verify they fail**

Run (from `backend/`): `cargo test`
Expected: FAIL — no `add` subcommand.

- [ ] **Step 3: Implement `add`**

`--description` and `--description-file` are mutually exclusive (clap
`conflicts_with`). `--description-file -` reads stdin to end; anything else is
a path. There is no implicit stdin reading — a non-TTY stdin alone does
nothing, so an agent can never leak an unrelated pipe into a task body. The
body passes through `normalize_markdown` before being sent. Success prints:

```
Created <id> in <Project> at the top of the stack: <title>
```

`--json` prints the created task JSON. HTTP 400 from the server (empty title,
unknown project id) maps to exit 1 with the server's message.

- [ ] **Step 4: Run tests and the gate**

Run (from `backend/`): `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: PASS.

- [ ] **Step 5: Human verification (required — shell-quoting behaviour differs per shell)**

With a real (non-development) instance running, from another repository. The
binary is still not on `PATH` (Phase 7 installs it), so either use the full
path to `backend\target\debug\taskcascade.exe` or run
`$env:PATH = "<repo>\backend\target\debug;$env:PATH"` for the session.

PowerShell (the here-string body must contain a fenced code block, a list, and
a blank line, so that fence handling is genuinely exercised):

````powershell
@'
# Heading

- item one
- item two

```rust
fn main() {}
```
'@ | taskcascade add --project Engine --title "Try multi-line" --description-file -
Get-Content .\notes.md | taskcascade add --project Engine --title "From file" --description-file -
taskcascade add --project Engine --title "From path" --description-file .\notes.md
````

bash (WSL or Git Bash):

````bash
taskcascade add --project Engine --title "Heredoc" --description-file - <<'EOF'
# Heading

```rust
fn main() {}
```
EOF
````

Then run `taskcascade show <id>` for each and open the UI: descriptions must
render as intended Markdown with no lost blank lines, no doubled newlines, and
intact code fences.

- [ ] **Step 6: Commit**

```powershell
git add -A && git commit -m "Create tasks from the command line with Markdown from stdin or a file"
```

---

### Phase 6: Frontend picks up agent-created tasks on window focus

`frontend/src/App.tsx:94-106` fetches `/api/bootstrap` once on mount and never
again, so tasks an agent creates stay invisible until a manual reload. The
refresh must be ordered against in-flight mutations: a bootstrap request
started before a mutation can resolve *after* it and would otherwise restore
stale state over a fresh edit.

**Files:**

- Modify: `frontend/src/App.tsx`.

- [ ] **Step 1: Give mutations a sequence and route them through one commit path**

Add three refs in `App`:

```tsx
const mutationSeq = useRef(0);   // bumped when a mutation starts
const inFlight = useRef(0);      // mutations awaiting a response
const refreshSeq = useRef(0);    // bumped when a refresh starts
```

Wrap every existing mutation helper (`createTask`, `updateTask`, `complete`,
`restore`, `moveToTrash`, `undelete`, `reorder`, and the project mutations) so
it increments `mutationSeq.current` and `inFlight.current` before awaiting the
API and, in a `finally`, decrements `inFlight.current` **and increments
`mutationSeq.current` again**. The completion bump closes the residual race: a
mutation that started before a refresh and completed while the bootstrap was
in flight would otherwise pass all three guards (sequence unchanged, nothing
in flight) and let a stale bootstrap revert the just-applied result. A small
`async function mutate<T>(run: () => Promise<T>, apply: (result: T) => void)`
helper keeps this to one line per handler and leaves the existing
`setData`/`showError` behaviour untouched.

- [ ] **Step 2: Add an ordered background refresh**

Extract the mount effect's body into `async function refresh(initial: boolean)`:

```tsx
const mine = ++refreshSeq.current;
const seenMutations = mutationSeq.current;
const boot = await api.bootstrap();
if (mine !== refreshSeq.current) return;                 // a newer refresh won
if (seenMutations !== mutationSeq.current) return;       // a mutation started or completed meanwhile
if (inFlight.current > 0) return;                        // a mutation is still landing
setData(boot);
```

Further rules:

- On the initial call only, apply the saved project filter and select the
  first active task (today's behaviour).
- On a background call, keep `view`, `selectedProjects`, and `selectedId`; the
  existing effect at `App.tsx:145-148` re-points the selection if the selected
  task vanished.
- Background failures are silent (`console.warn`) — they must not raise the
  error toast, which is reserved for user-initiated actions.
- Read the search query from a ref (`queryRef.current`, kept in sync with the
  `query` state), never from a captured closure variable, and re-run
  `api.search(queryRef.current)` when it is non-empty so the search view
  reflects new tasks. The same discard rule applies to the search response.
- Defer while the editor has an unsaved draft: `TaskEditor` gains
  `onPendingChange(pending: boolean)`, called `true` when it schedules its
  450 ms save and `false` when the save is issued; `App` holds
  `editorPending`. When `refresh(false)` finds it true it schedules exactly
  one retry 1 s later (guarded by a ref so retries cannot stack) instead of
  fetching.

- [ ] **Step 3: Wire the focus listener**

Add an effect with empty dependencies registering `window` `focus` and
`document` `visibilitychange` (acting only when
`document.visibilityState === "visible"`), both calling `refresh(false)`, with
a `useRef` timestamp guard that skips refreshes less than 2 s apart. Clean up
both listeners on unmount. Because every value `refresh` reads comes from refs
or from `setData`'s updater form, the listener never captures stale state.

Record in a short comment why the draft survives: the editor's draft lives in
`TaskEditor`'s own `useState` and the component is keyed by `selected.id`
(`App.tsx:597`), so replacing `data` re-renders without remounting; the
sequence guard above is what prevents a late bootstrap from undoing a save.
A future change that keys the editor differently must revisit this.

- [ ] **Step 4: Verify the gate**

Run (from `frontend/`): `npm run check` then `npm run fmt`
Expected: PASS. (The repository has no frontend test runner, so the ordering
rule is covered by the manual steps below; adding one is out of scope here.)

- [ ] **Step 5: Human verification (required — the whole point of the phase)**

With the app open in the browser and a terminal beside it:

1. `taskcascade add --project <name> --title "Focus test"`, click the browser
   window: the task appears at the top of the stack within a moment.
2. Type into a task description, alt-tab away and back **within a second**
   (several times), then wait: the typed text is still there, and a reload
   shows it persisted. This is the ordering race — if the text ever reverts,
   the sequence guard is wrong.
3. In devtools, throttle the network to "Slow 3G", type a character, alt-tab
   out and back immediately, and confirm the same.
4. Complete/drag operations still work after several focus cycles. Focus the
   window (triggering a refresh) and immediately complete a task, then again
   with a drag: neither result may be visually reverted when the background
   bootstrap lands — this exercises the mutation-completion bump from Step 1.
5. With the backend stopped, focus the window: no error toast appears and the
   UI keeps its data.

- [ ] **Step 6: Commit**

```powershell
git add -A && git commit -m "Refresh the task lists when the window regains focus"
```

---

### Phase 7: Agent-facing assets — skill, Codex pointer, installed binary

**Files:**

- Create: `agent-access/claude-skill/taskcascade/SKILL.md`,
  `agent-access/codex/AGENTS-snippet.md`, `agent-access/README.md` (a thin
  pointer), `scripts/install-agent-access.ps1`, `docs/AgentAccess.md`.

- [ ] **Step 1: Write `docs/AgentAccess.md` (the durable contract)**

Contents: what agent access is and is not (read plus create; no editing,
completing, reordering, or deleting); the command surface and flag defaults
from this plan's "Command surface" section; the two output formats with a
worked example of each; the exit-code table; how discovery works (registry
directory with heartbeat, health probe, refusal when the data directory is not
the expected one, `--port` as a registry-bypassing direct probe,
`TASKCASCADE_DATA_DIR` for deliberate targeting); the "not running" behaviour
and that the CLI never auto-starts anything; the add hygiene rules; and
installation — building the binary, where it is copied, the Claude Code skill,
and the Codex `AGENTS.md` line, including that the live copies sit outside this
repo (`~/.claude/skills/taskcascade/`, `~/.codex/AGENTS.md`) and must be
re-installed when the versioned text changes. State plainly that the install
step here is provisional and will be superseded by the packaging design's
`package.ps1`/`package.sh`, and note the Linux equivalent by hand
(`cargo build --release` and copy to `~/.local/bin`).

- [ ] **Step 2: Write the Claude Code skill**

`agent-access/claude-skill/taskcascade/SKILL.md` with YAML frontmatter (`name:
taskcascade`, a `description` that says when to use it — the user asks what to
work on next, asks about their backlog or a project's tasks, or asks to record
follow-up work) and a body covering:

- The five commands with one example each; **search, then show** for detail,
  and `--json` whenever output must be parsed.
- **Search before adding.** Always run `taskcascade search "<keywords>"`
  first and do not create a near-duplicate; if something similar exists, tell
  the user instead of adding.
- **Provenance line.** End every created description with a final line such as
  `Added by Claude Code from repo <repo-name> on <YYYY-MM-DD>.` (there is no
  attribution field in the data model — this is the only record).
- **Never create projects.** If the project is unknown, run
  `taskcascade projects` and ask the user which one to use.
- Exit code 2 means TaskCascade is not running (or the wrong instance was
  found): report it to the user and stop. Do not start a server, do not open
  the database, do not retry in a loop. Exit code 1 means the command was
  wrong (unknown project or id) — fix the arguments, do not retry blindly.
- A repository may name its home project in its own `Agents.md`; that is a
  hint, never a requirement.

- [ ] **Step 3: Write the Codex pointer**

`agent-access/codex/AGENTS-snippet.md` holds the few lines to paste into
`~/.codex/AGENTS.md`. It must be self-contained (that machine-global file
cannot rely on this repository being checked out): name the commands, the
search-before-add and provenance rules, "never create projects", and the
meanings of exit codes 1 and 2.

- [ ] **Step 4: Add the install script (skill *and* binary)**

`scripts/install-agent-access.ps1`, parameters `-BinDir <path>`, `-Force`,
`-SkipBuild`:

1. Unless `-SkipBuild`, run `cargo build --release` in `backend/`.
2. Copy `backend/target/release/taskcascade.exe` to `-BinDir` (default
   `$env:LOCALAPPDATA\Programs\TaskCascade`), creating the directory. If the
   destination file is locked, stop with a clear message telling the user to
   exit the running TaskCascade first — do not silently skip the copy.
3. If `-BinDir` is not on the **user** `PATH`: first save the current user
   `PATH` value to a backup file in `-BinDir` and print its location; if the
   appended value would exceed ~1900 characters, do not write — print manual
   instructions instead (some Windows APIs truncate longer values). Otherwise
   append with `[Environment]::SetEnvironmentVariable('Path', …, 'User')`
   (this rewrites the value as REG_SZ, freezing any `%VAR%` references — the
   backup exists to recover from that) and print that a new shell is needed
   for it to take effect.
4. Copy `agent-access/claude-skill/taskcascade` to
   `$HOME/.claude/skills/taskcascade`, refusing to overwrite an existing
   directory without `-Force`.
5. Print the exact Codex snippet and the path (`~/.codex/AGENTS.md`) for the
   user to paste it into. The script must **not** edit that file — it is
   user-owned prose.
6. Print a summary of everything it did, and a reminder that the binary copy
   is provisional plumbing until the packaging scripts exist.

- [ ] **Step 5: Human verification (required — external tooling)**

Run the install script, open a **new** shell, and confirm `taskcascade
projects` works from an unrelated directory with no path prefix. In a
*different* repository, ask Claude Code "what's next for <project>?" and "add a
task to <project> to …" and confirm it uses the CLI, searches first, uses
`show` for detail, and appends the provenance line. Repeat in Codex with the
pointer line installed. Finally, with TaskCascade stopped, confirm both agents
report "TaskCascade is not running" and do not attempt to start it or open the
database.

- [ ] **Step 6: Commit**

```powershell
git add -A && git commit -m "Add the agent skill, Codex pointer, and install script"
```

---

### Phase 8: Documentation reconciliation and final verification

**Files:**

- Modify: `docs/DecisionLog.md` (append), `docs/Design.HighLevel.md`,
  `docs/RequirementSpecification.md`, `README.md`.

- [ ] **Step 1: Append the decision-log entry**

Add at the end of `docs/DecisionLog.md` (never rewrite earlier entries), dated
the day the work lands:

```markdown
## 2026-08-29 — Agent access through CLI subcommands on the single binary

Decision: AI coding agents reach TaskCascade through subcommands on the same
`taskcascade` binary (`projects`, `search`, `list`, `show`, `add`), which act
as a short-lived loopback HTTP client against the running server; running the
binary with no subcommand still starts the application. Each running server
announces itself (port, pid, resolved data directory, refresh timestamp) in a
per-user registry directory and keeps that record fresh with a heartbeat;
`GET /api/health` reports the same data directory, and the client refuses to
act against an instance serving any other data directory. `--port` bypasses the
registry and probes a port directly for deliberate targeting. Agents may read
and create tasks only.
Context: Agents needed to find and record work without the browser, and the
approved single-binary packaging design promises exactly one distributed file.
A second binary would have reintroduced an install step and client/server
version skew, and the development instance from `scripts/start.ps1` runs on a
throwaway database that agents must never write to.
Consequences: The API gains lightweight read endpoints (`GET /api/projects`,
`GET /api/tasks`, `GET /api/tasks/{id}`, scoping and limits on
`GET /api/search`); search patterns are escaped so `_` and `%` are literal. The
client never opens SQLite, so the server remains the sole enforcer of the
ordering, archive, and trash invariants. Loopback-only still holds and no new
exposure is created — any local process could already reach the API. The UI
refetches on window focus, ordered against in-flight mutations so a background
refresh cannot undo an edit. Agent-facing text (Claude Code skill, Codex
pointer) is versioned in `agent-access/`, and the contract lives in
`docs/AgentAccess.md`; the live copies outside the repo must be re-installed
when it changes.
```

- [ ] **Step 2: Update `docs/Design.HighLevel.md`**

Add the client mode to the architecture description: the same binary runs
either the server or a short-lived CLI client; the instance registry, its
heartbeat, and the health data-directory field; the new read endpoints and the
fact that search is scoped/limited server-side and wildcard-safe; and that the
UI refetches on window focus so externally created tasks appear. Link
`docs/AgentAccess.md`.

- [ ] **Step 3: Update `docs/RequirementSpecification.md`**

Inside the file's outer ```` ```markdown ```` fence:

- Add an "Agent access" section under Functional Requirements: agents can list
  projects, search, show one task, list a project's stack, and create tasks
  from the command line; created tasks enter at the top of the stack like any
  other; no other mutation is available to agents; the tool never starts
  itself for an agent.
- **Resolve the contradiction** in the Technology section
  (`docs/RequirementSpecification.md:282-294`): "No server required." predates
  the local backend and now conflicts with agent commands, which require the
  local server to be running. Reword it to say that no *remote* server,
  hosting, or account is required — the application includes a local
  loopback-only process that owns the SQLite file — and keep "Local storage
  only".
- Leave the remaining MVP sections untouched.

- [ ] **Step 4: Update `README.md`**

Add a short "Agent access" section after "MVP features" pointing to
`docs/AgentAccess.md` and `scripts/install-agent-access.ps1`, and mention the
binary is now `taskcascade` (`taskcascade.exe` on Windows).

- [ ] **Step 5: Full verification**

Run (from `backend/`): `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Run (from `frontend/`): `npm run check` and `npm run fmt`
Expected: all pass with no formatting diffs.

- [ ] **Step 6: Human verification pass (required)**

1. `scripts/start.ps1` still runs the app end to end (create, edit, drag to
   Archive/Deleted, restore, search, reorder), and leaves no
   `TASKCASCADE_DATA_DIR` behind in the session.
2. From another repository with only the development instance running, every
   CLI command refuses with the data-directory message; adding `--port <dev
   port>` makes each one work.
3. With a real instance running, `projects`, `search`, `list`, `show`, and
   `add` all behave as documented, including `--json` and `--full`.
4. Stop TaskCascade: every command exits 2 with the "not running" message;
   after the record ages past the grace period it is cleaned up on the next
   attempt.
5. Two instances at once (real plus development): commands still pick the real
   one.
6. Restart the server on the same port and confirm the record is replaced (one
   file, new pid) rather than duplicated or stale.
7. Delete the registry record while the server runs: within a minute the
   heartbeat restores it, and `--port` works in the meantime.

- [ ] **Step 7: Commit**

```powershell
git add -A && git commit -m "Document agent access to TaskCascade"
```

---

## Documents this work updates

| Document | Why |
| --- | --- |
| `docs/DecisionLog.md` | New durable commitment: agent access surface, discovery/refusal rules, read-plus-create scope (appended entry, Phase 8). |
| `docs/AgentAccess.md` (new) | The lasting contract: commands, flags, output formats, exit codes, discovery, install notes. |
| `docs/Design.HighLevel.md` | Architecture now includes a client mode, an instance registry, new read endpoints, and focus refetch. |
| `docs/RequirementSpecification.md` | New "Agent access" requirement; the "No server required" line is clarified to mean no remote server. |
| `README.md` | Agent access section; renamed binary. |
| `agent-access/**` (new) | Versioned copies of the skill and Codex pointer that live outside the repo. |
| `scripts/start.ps1`, `scripts/install-agent-access.ps1` | Environment restore; provisional binary and skill installation. |

This repository keeps no requirement tags, changelog, or benchmark suite, so no
such artifacts are coupled to these phases, and no version bump accompanies the
work (the repository has no version policy; capability detection replaces
version comparison).

## Resolved questions

Every question from the previous revision has been answered by the user and is
now folded into the plan:

1. **Registry directory versus a single instance file** — the directory of
   per-port announcement files is approved (masking rationale accepted).
2. **Wrong-data-directory targeting** — `--port` is kept, and now bypasses the
   registry to probe the named port directly, so it works even when
   registration failed; an explicitly set `TASKCASCADE_DATA_DIR` remains the
   path used by tests and by deliberate development targeting.
3. **Version marker** — no bump; the repository has no version policy.
4. **Install-script reach** — the script installs the release binary to a PATH
   directory and the skill to `~/.claude/skills`, and only *prints* the Codex
   `AGENTS.md` line.
5. **`taskcascade show <id>`** — in v1 (Phase 4), with a new
   `GET /api/tasks/{id}` endpoint; the skill teaches "search, then show".
6. **HTTP client crate** — `ureq` with `default-features = false`; see the
   contingency below.

## Open Questions

1. **HTTP client contingency (implementer's call, Phase 2 Step 4).** If
   `ureq` with `default-features = false` turns out not to build cleanly
   HTTP-only, switch to `reqwest` with
   `default-features = false, features = ["blocking"]`. The `Transport` seam
   makes this a contained change; note the swap in the commit message rather
   than re-planning.
2. **Binary installation is provisional.** Phase 7 builds and copies the
   release binary and edits the user `PATH` because the packaging design's
   `package.ps1`/`package.sh` do not exist yet. When that design is
   implemented, `scripts/install-agent-access.ps1` should shrink to installing
   the skill text and pointing at the packaged artifact; whoever implements
   packaging should expect to revisit this script.
