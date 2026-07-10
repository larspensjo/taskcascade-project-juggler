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
