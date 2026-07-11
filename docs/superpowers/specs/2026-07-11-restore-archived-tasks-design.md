# Restore Archived Tasks to the Working Stack — Design

**Date:** 2026-07-11
**Status:** Approved

## Summary

Add the ability to restore a completed (archived) task back into the active
ordered stack. Viewing archived tasks already exists today (the **Archive**
sidebar view with a read-only detail pane). This feature adds only the
**restore** action.

The requirement specification already anticipates this:

- Under *Archive*: "Future versions may allow restoring archived tasks."
- Under *Future Ideas*: "Task restore from archive."

## Behaviour

- A restored task lands at the **top** of the active stack, consistent with how
  newly created tasks are inserted.
- Restore is triggered by a **"Restore to stack"** button in the archive detail
  pane, occupying the same slot where active tasks show the "Complete" button.
- No keyboard shortcut (button only for this iteration).
- The `completed_at` timestamp is **cleared** on restore — restoring a task
  makes it active again, and an active task has no completion timestamp (the
  schema uses `completed_at IS NULL` to mean "active"). No completion history is
  retained across the restore. Tasks that remain in the archive still preserve
  all of their timestamps; only the act of restoring drops `completed_at`.
  Preserving completion history through a restore is a separate future idea
  ("Task history") and out of scope here.

## Backend

### Route

Add one route next to the existing `complete` route in `router()`:

```
.route("/api/tasks/{id}/restore", post(restore_task))
```

### Handler: `restore_task`

Mirrors `complete_task` in reverse and reuses the top-insertion logic from
`create_task`. All mutations run inside a single transaction:

1. Bump every active task down by one position:
   `UPDATE tasks SET position = position + 1 WHERE completed_at IS NULL`
2. Clear the archive marker and place the task on top:
   `UPDATE tasks SET completed_at = NULL, position = 0, modified_at = ?
    WHERE id = ? AND completed_at IS NOT NULL`
3. If step 2 reports `rows_affected == 0`, roll back and return `404`. The
   `completed_at IS NOT NULL` predicate guards against restoring a task that is
   missing or already active (double-restore).
4. Commit, then fetch and return the refreshed `Task` as JSON, using the same
   `SELECT` column list as the other handlers.

Note on ordering: the position bump in step 1 also shifts the target task (it is
not yet active-only at that point, but it is archived, so the
`completed_at IS NULL` predicate excludes it). Step 2 explicitly sets its
position to 0, so no conflict arises.

### Errors

Reuse the existing `ApiError` type. `404` via `ApiError::not_found()` for the
missing/already-active case; database failures map through the existing
`From<sqlx::Error>` implementation.

## Frontend

### `api.ts`

Add to the `api` object:

```ts
restoreTask: (id: string) =>
  request<Task>(`/tasks/${id}/restore`, { method: "POST" }),
```

### `App.tsx`

Add a `restore(task: Task)` handler — the inverse of `complete()`:

- Call `api.restoreTask(task.id)`.
- On success, update state: remove the task from `archivedTasks` and prepend the
  returned task to `activeTasks`.
- Switch `view` to `"active"` and set `selectedId` to the restored task so the
  user sees it land on top of the stack.
- On failure, `showError`.

Pass `onRestore={restore}` into `TaskEditor`.

### `TaskEditor`

In the archived branch, add a **"Restore to stack"** button in the
`editor-meta` row, in the same position where the active branch renders the
"Complete" button. Wire a new `onRestore: (task: Task) => void` prop through the
component signature. Reuse existing button styling (e.g. the `complete-text`
class or an analogous class) so it visually matches the Complete affordance.

The restore action is a lifecycle operation on the whole task; the archived
task's individual fields (title, description, scratchpad, project) remain
read-only in the archive, as they are today. Restore does not open the fields
for editing — it returns the task to the active stack, where the normal editor
then applies.

## Documentation Reconciliation

Implementing restore changes the product contract that three durable documents
currently describe. These updates are part of this feature's scope, so that the
repository does not carry conflicting sources of truth about whether restore is
supported.

### `docs/DecisionLog.md`

Add a new dated entry (2026-07-11) that explicitly **supersedes** the no-restore
portion of the earlier 2026-07-10 archive decision. The entry records:

- Archived tasks can now be restored to the top of the active stack.
- Archived task **content** stays read-only; restore is a lifecycle action, not
  field editing.
- Completion-timestamp semantics: `completed_at` is cleared when a task is
  restored (an active task has no completion timestamp); tasks that remain in
  the archive keep all their timestamps.

Leave the rest of the 2026-07-10 decision intact — only the "cannot be restored
/ archive mutations are post-MVP" part is superseded.

### `docs/RequirementSpecification.md`

- **Archive** section: state that archived tasks can be restored to the active
  stack, while archived task fields remain read-only. Replace the "Future
  versions may allow restoring archived tasks" sentence.
- **Delete / Complete Task** / timestamp wording: clarify that the archive
  preserves all timestamps for tasks that remain archived, and that restoring a
  task clears its completion timestamp as it re-enters the active list.
- **Future Ideas**: remove "Task restore from archive" (now implemented); leave
  "Task history" in place, since retaining completion history across a restore
  is still future work.

## Testing

`app.rs` currently has no handler-level tests; only `domain.rs` has a unit test
for `relocate`. This feature introduces the **first backend integration test**
for the project.

The test spins up the `router()` against an in-memory SQLite database
(`sqlite::memory:`) and exercises the full flow via HTTP-level calls:

1. Create a project and two tasks (A on top, B below).
2. Complete task B → it leaves the active list and enters the archive.
3. Restore task B → assert:
   - It returns to the active list at `position == 0` (top).
   - Its `completedAt` is `null`.
   - The previously-top task A is now at `position == 1`.
4. Restore task B again → assert `404` (already active; not double-restored),
   **then re-fetch bootstrap and assert the stack is unchanged**: B still at
   `position == 0`, A still at `position == 1`, active order intact, archive
   still empty. The handler bumps every active position *before* it discovers
   the task is not restorable, so only a working transaction rollback keeps the
   rejected restore from silently shifting the whole active stack — this
   assertion is what actually verifies the rollback.
5. Restore a nonexistent ID → assert `404`, then re-fetch bootstrap and assert
   the active stack and archive are unchanged (the other documented `404` path,
   which also runs the position bump before rejecting).

Frontend verification is manual/end-to-end: run the app, complete a task, open
the Archive view, click "Restore to stack", and confirm the task reappears at
the top of the active stack and is selected.

## Out of Scope

- Retaining completion history or a completion audit trail.
- Keyboard shortcut for restore.
- Restore-to-bottom or a per-restore position prompt.
- Bulk restore.
