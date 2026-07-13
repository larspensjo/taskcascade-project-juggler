# Drag-and-Drop Archive, Restore, and Trash — Design

**Date:** 2026-07-13
**Status:** Approved

## Summary

Replace the archive/restore buttons with drag-and-drop gestures, extending
the drag interaction the stack already uses for reordering. Task rows can be
dragged onto sidebar items: **Archive** to complete a task, a new **Deleted**
(trash) item to soft-delete it, and **All tasks** to bring an archived or
deleted task back. Deleted tasks are never destroyed — they live in a new
trash view, invisible everywhere else, and can be dragged back out.

The ✓ button on task rows and the "Complete" / "Restore to stack" buttons in
the editor are removed. The Delete-key shortcut keeps working as the keyboard
path for archiving in the active view.

## Goals and Non-Goals

Goals:

- Archive, restore, delete, and undelete all work as drag gestures onto
  sidebar drop targets that highlight when a valid drag hovers over them.
- Soft delete: a third task state alongside active and archived, with its
  own sidebar view. Deleted tasks are excluded from search, reorder, and all
  other operations.
- Undelete destination is chosen by drop target: **All tasks** puts the task
  on top of the active stack; **Archive** files it as completed.
- Row drag-to-reorder in the active stack is unchanged.

Non-goals (out of scope):

- Permanent deletion ("empty trash", auto-purge). The trash only grows;
  purging can be added later if needed.
- Keyboard equivalents for restore/undelete (restore becomes mouse-only).
- Archiving or deleting from the search view (today the editor's Complete
  button allowed it; after this change the user navigates to the stack
  first).
- Touch/pointer-event drag support beyond what native HTML5 drag-and-drop
  provides.

## Task States

A task is in exactly one state:

| State    | `completed_at` | `deleted_at` |
| -------- | -------------- | ------------ |
| active   | NULL           | NULL         |
| archived | set            | NULL         |
| deleted  | NULL           | set          |

Deleting clears `completed_at` (the drop target chooses the destination on
undelete, so the prior state need not be preserved). Consequence: undeleting
to the archive stamps a fresh `completed_at`, not the original one.

## Interaction Design

Drop targets are the existing sidebar items plus a new one; each highlights
on drag-over only when the dragged task is valid for it:

- **Archive** ← active task (completes it) or deleted task (undelete into
  the archive).
- **Deleted** (new, trash icon, below Archive, with count) ← active or
  archived task (moves it to the trash).
- **All tasks** ← archived task (restore to top of stack, keeping its
  project) or deleted task (undelete to top of stack).

Projects in the sidebar are not drop targets. Dropping anywhere that is not
a valid target cancels the drag (native behavior; Escape also cancels).

Rows are draggable in the active, archive, and deleted views. Dropping on
another *row* reorders, and only in the active view. Search results are not
draggable.

The **Deleted** sidebar item opens a trash view that mirrors the archive
view: same list layout, read-only editor showing "Deleted \<date\>" in the
meta line, description and scratchpad rendered read-only.

Removed affordances: the ✓ complete button on each task row, and the
"Complete" and "Restore to stack" buttons in the editor. Kept: the Delete
key completes the selected task in the active view.

## Backend Changes

### Migration

`0003_add_task_deleted_at.sql`: `ALTER TABLE tasks ADD COLUMN deleted_at
TEXT;` (NULL for all existing rows.)

### Model and bootstrap

- `Task` gains `deleted_at: Option<String>` (serialized `deletedAt`).
- `Bootstrap` gains `deleted_tasks` (`deletedTasks`), fetched with
  `deleted_at IS NOT NULL ORDER BY deleted_at DESC`.
- `fetch_tasks` predicates become: active `completed_at IS NULL AND
  deleted_at IS NULL`; archived `completed_at IS NOT NULL AND deleted_at IS
  NULL`.

### New endpoints

- `POST /api/tasks/{id}/delete` — guard `deleted_at IS NULL`; sets
  `deleted_at = now`, `completed_at = NULL`, `modified_at = now`. Accepts
  active and archived tasks; 404 if already deleted or nonexistent.
- `POST /api/tasks/{id}/undelete` with body `{ "to": "stack" | "archive" }`
  — guard `deleted_at IS NOT NULL`; 404 otherwise.
  - `to = "stack"`: in one transaction, shift all active positions +1, then
    clear `deleted_at`, set `position = 0`, `modified_at = now` (mirrors
    `restore_task`).
  - `to = "archive"`: clear `deleted_at`, set `completed_at = now`,
    `modified_at = now`.
  - Any other `to` value: 400.

### Existing endpoints exclude deleted tasks

Add `deleted_at IS NULL` to the guards of `update_task`, `complete_task`,
and `restore_task`, to the id list in `reorder_task`, and to the `search`
query. Deleted tasks are untouchable except through `undelete`.

Positions: a deleted active task leaves a gap in the position sequence,
exactly as completing one does today; `relocate` already tolerates gaps and
reorder rewrites positions densely.

## Frontend Changes

### `api.ts`

- `Task` gains `deletedAt: string | null`; `Bootstrap` gains
  `deletedTasks: Task[]`.
- New calls: `deleteTask(id)` and `undeleteTask(id, to: "stack" |
  "archive")`.

### `App.tsx`

- `View` gains `"deleted"`. `visibleTasks` sources `deletedTasks` for it;
  pane heading shows "Trash" / "Deleted".
- Drag state lifts from `TaskList` to `App` (`dragged: Task | null`), set
  via `onDragStart`/`onDragEnd` callbacks. Row-drop reorder logic stays in
  `TaskList`.
- Sidebar items get `onDragOver`/`onDrop` handlers gated by the dragged
  task's state (see Interaction Design), plus a drop-highlight class while
  a valid drag hovers. Drop calls `complete`, `restore`, or the new
  `deleteTask`/`undeleteTask` state helpers, which move the task between
  the three lists in client state (deleted list is newest-first).
- New **Deleted** nav item (lucide `Trash2`) below Archive with count.
- `TaskList`: `canReorder` splits into `draggable` (active, archive, and
  deleted views) and `canReorder` (active view only, gates row-drop
  reordering); the ✓ complete button is removed.
- `TaskEditor`: "Complete" and "Restore to stack" buttons removed. The
  read-only rendering used for archived tasks also serves deleted tasks,
  with "Deleted \<date\>" in the meta line.
- Keyboard handling unchanged (Delete completes in active view).

### `styles.css`

Drop-target highlight style for sidebar items; remove now-unused ✓ button
styles.

## Documentation Reconciliation

- **`docs/RequirementSpecification.md`:** the "Delete / Complete Task" and
  "Archive" sections describe button-driven completion and restore; update
  them to describe the drag gestures, the trash state, and its rules
  (deleted tasks excluded from search; undelete destination chosen by drop
  target; no purge). Keep the Delete-key shortcut row.
- **`docs/DecisionLog.md`:** dated entry (2026-07-13): drag-and-drop chosen
  over buttons for archive/restore/delete; soft delete via `deleted_at`
  with `completed_at` cleared so the drop target picks the undelete
  destination; no purge mechanism for now; search excludes deleted tasks.
- **`README.md`:** feature list mentions "Completion to a read-only
  searchable archive" — extend with the trash/soft-delete feature.

## Testing

Backend (new integration test file alongside `backend/tests/restore.rs`):

- Delete an active task: it leaves `activeTasks`, appears in
  `deletedTasks`, `completedAt` is null, `deletedAt` set.
- Delete an archived task: leaves `archivedTasks`, `completedAt` cleared.
- Undelete to stack: task at position 0, others shifted, `deletedAt`
  cleared; rejected undelete (already active) is a 404 that does not shift
  the stack (transaction rollback, mirroring the restore test).
- Undelete to archive: `completedAt` freshly set, `deletedAt` cleared.
- Invalid `to` value: 400. Double delete / unknown id: 404.
- Deleted tasks excluded: search does not match them; `update`, `complete`,
  `restore`, `reorder` against a deleted task return 404 / skip it.

Frontend: no test framework exists; `npm run check` (tsc + biome) must
pass.

Manual verification (via the running app):

1. Drag a task from the stack onto Archive — it completes; Archive
   highlights during hover; the count updates.
2. Drag a task onto Deleted — it moves to the trash; repeat from the
   archive view.
3. Drag from the archive onto All tasks — it returns to the top of the
   stack with its project intact.
4. Drag from the trash onto All tasks and onto Archive — lands active on
   top, respectively archived with a fresh completion date.
5. Row reordering in the stack still works; dropping a row on a project or
   empty space does nothing.
6. Invalid targets do not highlight (e.g., dragging an active task over All
   tasks).
7. Delete key still completes in the active view; search finds active and
   archived but not deleted tasks.
