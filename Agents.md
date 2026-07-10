# Repo Instructions

## Workflow

- Run backend commands from `backend/` and frontend commands from `frontend/`.
- For backend changes, run `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt`.
- For frontend changes, run `npm run check` and `npm run fmt`.
- Use `scripts/start.ps1` for the local development experience. It keeps the Rust API and Vite dev server coordinated.
- The UI follows the calm, compact dark-token approach recorded in `docs/VisualDesign.DarkTheme.md`; use CSS tokens rather than inline colours.

## Architecture

- The application is local-first: SQLite is the data of record and the Rust binary exposes a loopback HTTP API.
- Preserve the ordered active list invariant: task position is the only priority. Completed tasks are archived and must remain immutable from the UI/API.
- Keep pure ordering or search derivation in the domain layer so it remains unit-testable; keep HTTP and SQL code in the app layer.
- Keep React input flow predictable: user input → local draft → API mutation → refreshed application state → render. Isolate API calls in `frontend/src/api.ts`.
- Markdown is content, not HTML. Do not enable raw HTML rendering without an explicit security decision.

## Decisions and documentation

- `docs/DecisionLog.md` is the durable record of non-obvious commitments. Add concise, new entries at the end; do not rewrite old entries.
- Plans and design notes may be placed under `docs/` when they express a lasting decision or contract. Keep implementation milestones out of durable docs.

## Testing

- Add focused regression tests for bugs where practical.
- Prefer public behavior, pure ordering logic, and API contracts over implementation-detail tests.
