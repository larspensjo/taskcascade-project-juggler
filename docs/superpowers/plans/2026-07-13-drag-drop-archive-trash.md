# Drag-and-Drop Archive, Restore, and Trash — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace archive/restore buttons with drag-and-drop onto sidebar targets, and add a soft-delete trash (new `deleted` task state, trash view, drag in/out).

**Architecture:** A new nullable `deleted_at` column makes tasks active / archived / deleted (deleting clears `completed_at`; the undelete drop target chooses the destination). Two new endpoints (`delete`, `undelete`) mirror `complete`/`restore`; every other query gains `deleted_at IS NULL`. The frontend lifts the existing HTML5 drag state from `TaskList` to `App` so the sidebar items become drop targets.

**Tech Stack:** Rust (axum, sqlx/SQLite), React 19 + TypeScript (native HTML5 drag-and-drop, no new libraries).

**Spec:** `docs/superpowers/specs/2026-07-13-drag-drop-archive-trash-design.md`

## Global Constraints

- Run backend commands from `backend/`, frontend commands from `frontend/` (per `Agents.md`).
- Backend gate: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt`.
- Frontend gate: `npm run check`, `npm run fmt`.
- Task states are exclusive: active (`completed_at` NULL, `deleted_at` NULL), archived (`completed_at` set, `deleted_at` NULL), deleted (`deleted_at` set, `completed_at` NULL).
- The undelete request body destination values are exactly `"stack"` and `"archive"`.
- No purge mechanism. No new dependencies. UI colors come from CSS tokens, not inline values.
- Commit messages follow repo style: short imperative sentence, no `feat:` prefixes (see `git log`).

---

### Task 1: Backend — `deleted_at` column, Task model, bootstrap `deletedTasks`

**Files:**

- Create: `backend/migrations/0003_add_task_deleted_at.sql`
- Modify: `backend/src/app.rs` (Task struct ~line 141, Bootstrap struct ~line 155, `bootstrap` ~line 170, `fetch_tasks` ~line 189, `create_task` Task literal ~line 295, every `SELECT ... FROM tasks` column list)
- Test: `backend/tests/trash.rs` (new)

**Interfaces:**

- Produces: `Task` JSON gains `"deletedAt": string | null`; bootstrap JSON gains `"deletedTasks": Task[]` (ordered `deleted_at DESC`). `fetch_tasks(pool, filter: TaskFilter)` with `enum TaskFilter { Active, Archived, Deleted }`. Test helpers in `trash.rs`: `setup()`, `send()`, `bootstrap()`, `create_project()`, `create_task()` — Tasks 2–4 add tests to this file and reuse them.

- [ ] **Step 1: Write the failing test**

Create `backend/tests/trash.rs` with the shared helpers (same pattern as `backend/tests/restore.rs`) and the first test:

```rust
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use taskcascade_backend::app::{router, AppState};
use tower::ServiceExt;

async fn setup() -> Router {
    let state = AppState::connect("sqlite::memory:")
        .await
        .expect("in-memory database should connect");
    router(state)
}

async fn send(app: &Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(match body {
            Some(value) => Body::from(value.to_string()),
            None => Body::empty(),
        })
        .expect("request should build");
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("request should not fail at the transport level");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

async fn bootstrap(app: &Router) -> Value {
    let (status, body) = send(app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    body
}

async fn create_project(app: &Router) -> String {
    let (status, project) = send(
        app,
        "POST",
        "/api/projects",
        Some(json!({ "name": "Engine" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    project["id"].as_str().expect("project id").to_owned()
}

async fn create_task(app: &Router, project_id: &str, title: &str) -> String {
    let (status, task) = send(
        app,
        "POST",
        "/api/tasks",
        Some(json!({ "title": title, "projectId": project_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    task["id"].as_str().expect("task id").to_owned()
}

#[tokio::test]
async fn bootstrap_exposes_deleted_tasks_list() {
    let app = setup().await;
    let project_id = create_project(&app).await;
    let (status, task) = send(
        &app,
        "POST",
        "/api/tasks",
        Some(json!({ "title": "A", "projectId": project_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // The field must be present-and-null, not merely absent.
    assert!(matches!(task.get("deletedAt"), Some(Value::Null)));

    let boot = bootstrap(&app).await;
    let deleted = boot["deletedTasks"].as_array().expect("deletedTasks array");
    assert!(deleted.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run (from `backend/`): `cargo test --test trash`
Expected: FAIL — `bootstrap_exposes_deleted_tasks_list` panics (`deletedAt` absent from the task JSON, and `deletedTasks array` missing from bootstrap).

- [ ] **Step 3: Add the migration**

Create `backend/migrations/0003_add_task_deleted_at.sql`:

```sql
ALTER TABLE tasks ADD COLUMN deleted_at TEXT;
```

- [ ] **Step 4: Extend the model and bootstrap in `backend/src/app.rs`**

Add `deleted_at` to the Task struct:

```rust
#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
struct Task {
    id: String,
    title: String,
    description: String,
    scratchpad: String,
    project_id: String,
    position: i64,
    created_at: String,
    modified_at: String,
    completed_at: Option<String>,
    deleted_at: Option<String>,
}
```

Add `deleted_tasks` to Bootstrap:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bootstrap {
    projects: Vec<Project>,
    active_tasks: Vec<Task>,
    archived_tasks: Vec<Task>,
    deleted_tasks: Vec<Task>,
    preferences: Vec<Preference>,
}
```

Replace the `bool` filter on `fetch_tasks` with a three-way enum and updated predicates:

```rust
enum TaskFilter {
    Active,
    Archived,
    Deleted,
}

async fn fetch_tasks(pool: &SqlitePool, filter: TaskFilter) -> Result<Vec<Task>, sqlx::Error> {
    let (predicate, order) = match filter {
        TaskFilter::Active => ("completed_at IS NULL AND deleted_at IS NULL", "position ASC"),
        TaskFilter::Archived => (
            "completed_at IS NOT NULL AND deleted_at IS NULL",
            "completed_at DESC",
        ),
        TaskFilter::Deleted => ("deleted_at IS NOT NULL", "deleted_at DESC"),
    };
    sqlx::query_as::<_, Task>(&format!(
        "SELECT id, title, description, scratchpad, project_id, position, created_at, modified_at, completed_at, deleted_at FROM tasks WHERE {predicate} ORDER BY {order}"
    )).fetch_all(pool).await
}
```

Update the `bootstrap` handler to use the enum and fetch the third list:

```rust
    let active_tasks = fetch_tasks(&state.0, TaskFilter::Active).await?;
    let archived_tasks = fetch_tasks(&state.0, TaskFilter::Archived).await?;
    let deleted_tasks = fetch_tasks(&state.0, TaskFilter::Deleted).await?;
```

and include `deleted_tasks` in the `Bootstrap { .. }` literal.

In `create_task`, add `deleted_at: None,` to the `Task { .. }` literal (the INSERT statement needs no change — the column defaults to NULL).

**Every remaining `SELECT ... FROM tasks` used with `query_as::<_, Task>` must add `, deleted_at` to its column list** or `FromRow` fails at runtime. There are four, all currently reading `... modified_at, completed_at FROM tasks WHERE id = ?` — in `update_task`, `complete_task`, `restore_task`, and `search`. Change each to `... modified_at, completed_at, deleted_at FROM tasks ...`.

- [ ] **Step 5: Run tests to verify they pass**

Run (from `backend/`): `cargo test`
Expected: PASS — the new test and all existing tests (`restore.rs` asserts on fields that still exist).

- [ ] **Step 6: Lint, format, commit**

```powershell
cargo clippy --all-targets -- -D warnings && cargo fmt
git add -A && git commit -m "Add deleted_at task state and deletedTasks bootstrap list"
```

---

### Task 2: Backend — `POST /api/tasks/{id}/delete`

**Files:**

- Modify: `backend/src/app.rs` (router ~line 103, new handler next to `complete_task`)
- Test: `backend/tests/trash.rs`

**Interfaces:**

- Consumes: Task 1's helpers in `trash.rs`; `deleted_at` column.
- Produces: `POST /api/tasks/{id}/delete` → 200 with the updated Task JSON (`deletedAt` set, `completedAt` null); 404 if the task is already deleted or does not exist. Accepts active and archived tasks.

- [ ] **Step 1: Write the failing tests**

Append to `backend/tests/trash.rs`:

```rust
#[tokio::test]
async fn delete_moves_active_task_to_trash() {
    let app = setup().await;
    let project_id = create_project(&app).await;
    let task_id = create_task(&app, &project_id, "A").await;

    let (status, deleted) =
        send(&app, "POST", &format!("/api/tasks/{task_id}/delete"), None).await;
    assert_eq!(status, StatusCode::OK, "delete should succeed: {deleted}");
    assert!(deleted["deletedAt"].is_string());
    assert!(deleted["completedAt"].is_null());

    let boot = bootstrap(&app).await;
    assert!(boot["activeTasks"].as_array().expect("activeTasks").is_empty());
    let trash = boot["deletedTasks"].as_array().expect("deletedTasks");
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0]["id"], *task_id);
}

#[tokio::test]
async fn delete_clears_completed_at_on_archived_task() {
    let app = setup().await;
    let project_id = create_project(&app).await;
    let task_id = create_task(&app, &project_id, "A").await;
    let (status, _) = send(&app, "POST", &format!("/api/tasks/{task_id}/complete"), None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, deleted) =
        send(&app, "POST", &format!("/api/tasks/{task_id}/delete"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(deleted["deletedAt"].is_string());
    assert!(deleted["completedAt"].is_null());

    let boot = bootstrap(&app).await;
    assert!(boot["archivedTasks"].as_array().expect("archivedTasks").is_empty());
    assert_eq!(boot["deletedTasks"].as_array().expect("deletedTasks").len(), 1);
}

#[tokio::test]
async fn delete_rejects_deleted_and_unknown_tasks() {
    let app = setup().await;
    let project_id = create_project(&app).await;
    let task_id = create_task(&app, &project_id, "A").await;
    let (status, _) = send(&app, "POST", &format!("/api/tasks/{task_id}/delete"), None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(&app, "POST", &format!("/api/tasks/{task_id}/delete"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &app,
        "POST",
        "/api/tasks/00000000-0000-0000-0000-000000000000/delete",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run (from `backend/`): `cargo test --test trash`
Expected: the three new tests FAIL with status 404 (route does not exist) versus expected 200.

- [ ] **Step 3: Implement the handler**

In `backend/src/app.rs`, register the route after the `complete` route:

```rust
        .route("/api/tasks/{id}/complete", post(complete_task))
        .route("/api/tasks/{id}/delete", post(delete_task))
```

Add the handler after `complete_task`:

```rust
async fn delete_task(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Task>, ApiError> {
    let deleted_at = now();
    // completed_at is cleared: the undelete drop target chooses the
    // destination, so the pre-delete state is deliberately not preserved.
    let result = sqlx::query(
        "UPDATE tasks SET deleted_at = ?, completed_at = NULL, modified_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&deleted_at)
    .bind(&deleted_at)
    .bind(&id)
    .execute(state.0.as_ref())
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    let task = sqlx::query_as::<_, Task>("SELECT id, title, description, scratchpad, project_id, position, created_at, modified_at, completed_at, deleted_at FROM tasks WHERE id = ?")
        .bind(id).fetch_one(state.0.as_ref()).await?;
    Ok(Json(task))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run (from `backend/`): `cargo test`
Expected: PASS (all tests).

- [ ] **Step 5: Lint, format, commit**

```powershell
cargo clippy --all-targets -- -D warnings && cargo fmt
git add -A && git commit -m "Soft-delete endpoint moves tasks to the trash"
```

---

### Task 3: Backend — `POST /api/tasks/{id}/undelete`

**Files:**

- Modify: `backend/src/app.rs` (router, new handler next to `restore_task`)
- Test: `backend/tests/trash.rs`

**Interfaces:**

- Consumes: Task 2's delete endpoint (tests set up trashed tasks with it).
- Produces: `POST /api/tasks/{id}/undelete` with body `{ "to": "stack" | "archive" }` → 200 with updated Task JSON. `"stack"`: task at position 0, active tasks shifted +1, `deletedAt` null. `"archive"`: `completedAt` freshly stamped, `deletedAt` null. 400 for any other `to`; 404 if the task is not deleted.

- [ ] **Step 1: Write the failing tests**

Append to `backend/tests/trash.rs`:

```rust
#[tokio::test]
async fn undelete_to_stack_restores_to_top() {
    let app = setup().await;
    let project_id = create_project(&app).await;
    // B created first, then A, so A sits on top (position 0).
    let b_id = create_task(&app, &project_id, "B").await;
    let a_id = create_task(&app, &project_id, "A").await;
    let (status, _) = send(&app, "POST", &format!("/api/tasks/{b_id}/delete"), None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, undeleted) = send(
        &app,
        "POST",
        &format!("/api/tasks/{b_id}/undelete"),
        Some(json!({ "to": "stack" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "undelete should succeed: {undeleted}");
    assert_eq!(undeleted["position"], 0);
    assert!(undeleted["deletedAt"].is_null());
    assert!(undeleted["completedAt"].is_null());

    let boot = bootstrap(&app).await;
    let active = boot["activeTasks"].as_array().expect("activeTasks");
    assert_eq!(active.len(), 2);
    assert_eq!(active[0]["id"], *b_id);
    assert_eq!(active[0]["position"], 0);
    assert_eq!(active[1]["id"], *a_id);
    assert_eq!(active[1]["position"], 1);
    assert!(boot["deletedTasks"].as_array().expect("deletedTasks").is_empty());
}

#[tokio::test]
async fn undelete_to_archive_stamps_fresh_completed_at() {
    let app = setup().await;
    let project_id = create_project(&app).await;
    let task_id = create_task(&app, &project_id, "A").await;
    let (status, _) = send(&app, "POST", &format!("/api/tasks/{task_id}/delete"), None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, undeleted) = send(
        &app,
        "POST",
        &format!("/api/tasks/{task_id}/undelete"),
        Some(json!({ "to": "archive" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(undeleted["completedAt"].is_string());
    assert!(undeleted["deletedAt"].is_null());

    let boot = bootstrap(&app).await;
    assert!(boot["activeTasks"].as_array().expect("activeTasks").is_empty());
    assert_eq!(boot["archivedTasks"].as_array().expect("archivedTasks").len(), 1);
    assert!(boot["deletedTasks"].as_array().expect("deletedTasks").is_empty());
}

#[tokio::test]
async fn undelete_rejects_bad_destination_and_non_deleted_tasks() {
    let app = setup().await;
    let project_id = create_project(&app).await;
    let b_id = create_task(&app, &project_id, "B").await;
    let a_id = create_task(&app, &project_id, "A").await;
    let (status, _) = send(&app, "POST", &format!("/api/tasks/{b_id}/delete"), None).await;
    assert_eq!(status, StatusCode::OK);

    // Unknown destination is a 400.
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/tasks/{b_id}/undelete"),
        Some(json!({ "to": "somewhere" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Undeleting a task that is not deleted is a 404, and the rejected
    // request must not shift the stack (transaction rollback).
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/tasks/{a_id}/undelete"),
        Some(json!({ "to": "stack" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let boot = bootstrap(&app).await;
    let active = boot["activeTasks"].as_array().expect("activeTasks");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0]["id"], *a_id);
    assert_eq!(active[0]["position"], 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run (from `backend/`): `cargo test --test trash`
Expected: the three new tests FAIL with 404 (route missing) versus expected 200/400.

- [ ] **Step 3: Implement the handler**

Register the route after the `delete` route:

```rust
        .route("/api/tasks/{id}/delete", post(delete_task))
        .route("/api/tasks/{id}/undelete", post(undelete_task))
```

Add after `restore_task`:

```rust
#[derive(Deserialize)]
struct UndeleteTask {
    to: String,
}

async fn undelete_task(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<UndeleteTask>,
) -> Result<Json<Task>, ApiError> {
    match input.to.as_str() {
        "stack" => {
            let mut tx = state.0.begin().await?;
            sqlx::query(
                "UPDATE tasks SET position = position + 1 WHERE completed_at IS NULL AND deleted_at IS NULL",
            )
            .execute(&mut *tx)
            .await?;
            let result = sqlx::query(
                "UPDATE tasks SET deleted_at = NULL, position = 0, modified_at = ? WHERE id = ? AND deleted_at IS NOT NULL",
            )
            .bind(now())
            .bind(&id)
            .execute(&mut *tx)
            .await?;
            if result.rows_affected() == 0 {
                return Err(ApiError::not_found());
            }
            tx.commit().await?;
        }
        "archive" => {
            let timestamp = now();
            let result = sqlx::query(
                "UPDATE tasks SET deleted_at = NULL, completed_at = ?, modified_at = ? WHERE id = ? AND deleted_at IS NOT NULL",
            )
            .bind(&timestamp)
            .bind(&timestamp)
            .bind(&id)
            .execute(state.0.as_ref())
            .await?;
            if result.rows_affected() == 0 {
                return Err(ApiError::not_found());
            }
        }
        _ => {
            return Err(ApiError::bad_request(
                "Destination must be \"stack\" or \"archive\".",
            ))
        }
    }
    let task = sqlx::query_as::<_, Task>("SELECT id, title, description, scratchpad, project_id, position, created_at, modified_at, completed_at, deleted_at FROM tasks WHERE id = ?")
        .bind(id).fetch_one(state.0.as_ref()).await?;
    Ok(Json(task))
}
```

(Dropping `tx` without commit on the 404 path rolls the position shift back — same pattern as `restore_task`.)

- [ ] **Step 4: Run tests to verify they pass**

Run (from `backend/`): `cargo test`
Expected: PASS (all tests).

- [ ] **Step 5: Lint, format, commit**

```powershell
cargo clippy --all-targets -- -D warnings && cargo fmt
git add -A && git commit -m "Undelete endpoint returns trashed tasks to stack or archive"
```

---

### Task 4: Backend — deleted tasks are invisible to every other operation

**Files:**

- Modify: `backend/src/app.rs` (`create_task` shift query, `update_task`, `complete_task`, `restore_task`, `reorder_task`, `search`)
- Test: `backend/tests/trash.rs`

**Interfaces:**

- Consumes: delete endpoint from Task 2.
- Produces: `update`/`complete`/`restore` on a deleted task → 404; deleted tasks absent from search results and from the reorder id list; position-shift statements no longer touch deleted rows.

- [ ] **Step 1: Write the failing test**

Append to `backend/tests/trash.rs`:

```rust
#[tokio::test]
async fn deleted_tasks_are_excluded_from_mutations_search_and_reorder() {
    let app = setup().await;
    let project_id = create_project(&app).await;
    let b_id = create_task(&app, &project_id, "B needle").await;
    let a_id = create_task(&app, &project_id, "A").await;
    let (status, _) = send(&app, "POST", &format!("/api/tasks/{b_id}/delete"), None).await;
    assert_eq!(status, StatusCode::OK);

    // Editing a deleted task is a 404.
    let (status, _) = send(
        &app,
        "PUT",
        &format!("/api/tasks/{b_id}"),
        Some(json!({
            "title": "B2", "projectId": project_id,
            "description": "", "scratchpad": ""
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Completing and restoring a deleted task are 404s.
    let (status, _) = send(&app, "POST", &format!("/api/tasks/{b_id}/complete"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&app, "POST", &format!("/api/tasks/{b_id}/restore"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Search does not see trashed content.
    let (status, results) = send(&app, "GET", "/api/search?q=needle", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(results.as_array().expect("search results").is_empty());

    // Reordering against a deleted target is a 404 (it is not in the stack).
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/tasks/{a_id}/reorder"),
        Some(json!({ "targetTaskId": b_id, "after": true })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run (from `backend/`): `cargo test --test trash`
Expected: FAIL — update/complete return 200 for the deleted task and search finds "needle".

- [ ] **Step 3: Add `deleted_at IS NULL` guards**

In `backend/src/app.rs`, make these exact predicate changes:

- `create_task` position shift:
  `UPDATE tasks SET position = position + 1 WHERE completed_at IS NULL AND deleted_at IS NULL`
- `update_task`:
  `... WHERE id = ? AND completed_at IS NULL AND deleted_at IS NULL`
- `complete_task`:
  `... WHERE id = ? AND completed_at IS NULL AND deleted_at IS NULL`
- `restore_task` position shift:
  `UPDATE tasks SET position = position + 1 WHERE completed_at IS NULL AND deleted_at IS NULL`
- `restore_task` main update:
  `... WHERE id = ? AND completed_at IS NOT NULL AND deleted_at IS NULL`
- `reorder_task` id list:
  `SELECT id FROM tasks WHERE completed_at IS NULL AND deleted_at IS NULL ORDER BY position`
- `search`:
  `... FROM tasks WHERE deleted_at IS NULL AND (title LIKE ? OR description LIKE ? OR scratchpad LIKE ?) ORDER BY completed_at IS NOT NULL, position, completed_at DESC`

- [ ] **Step 4: Run tests to verify they pass**

Run (from `backend/`): `cargo test`
Expected: PASS (all tests, including `restore.rs`).

- [ ] **Step 5: Lint, format, commit**

```powershell
cargo clippy --all-targets -- -D warnings && cargo fmt
git add -A && git commit -m "Deleted tasks are excluded from search, reorder, and mutations"
```

---

### Task 5: Frontend — API client for delete/undelete

**Files:**

- Modify: `frontend/src/api.ts`

**Interfaces:**

- Consumes: Tasks 2–3 endpoints.
- Produces: `Task.deletedAt: string | null`; `Bootstrap.deletedTasks: Task[]`; `api.deleteTask(id: string): Promise<Task>`; `api.undeleteTask(id: string, to: "stack" | "archive"): Promise<Task>`. Tasks 6–7 consume these exact names.

- [ ] **Step 1: Extend the types and API object**

In `frontend/src/api.ts`, add to the `Task` type after `completedAt`:

```ts
  completedAt: string | null;
  deletedAt: string | null;
```

Add to the `Bootstrap` type after `archivedTasks`:

```ts
  archivedTasks: Task[];
  deletedTasks: Task[];
```

Add to the `api` object after `restoreTask`:

```ts
  deleteTask: (id: string) =>
    request<Task>(`/tasks/${id}/delete`, { method: "POST" }),
  undeleteTask: (id: string, to: "stack" | "archive") =>
    request<Task>(`/tasks/${id}/undelete`, {
      method: "POST",
      body: JSON.stringify({ to }),
    }),
```

- [ ] **Step 2: Verify the frontend gate**

Run (from `frontend/`): `npm run check`
Expected: FAIL is acceptable **only** if it flags `deletedTasks` missing where `Bootstrap` is consumed — it should actually PASS, because `App.tsx` only reads (never constructs) `Bootstrap`. If anything else fails, fix it before continuing.

- [ ] **Step 3: Format and commit**

```powershell
npm run fmt
git add -A && git commit -m "API client gains deleteTask and undeleteTask"
```

---

### Task 6: Frontend — Deleted view (sidebar item, trash list, read-only editor)

**Files:**

- Modify: `frontend/src/App.tsx`

**Interfaces:**

- Consumes: `api.deleteTask` / `api.undeleteTask` from Task 5.
- Produces: `View` includes `"deleted"`; `type TaskStatus = "active" | "archived" | "deleted"` and `statusOf(task: Task): TaskStatus` (module level); App functions `moveToTrash(task: Task)` and `undelete(task: Task, to: "stack" | "archive")`; `TaskEditor` prop `status: TaskStatus` (replaces `archived: boolean`). Task 7 consumes `statusOf`, `moveToTrash`, `undelete`.

- [ ] **Step 1: Add the status helper and view plumbing**

In `frontend/src/App.tsx`:

Add `Trash2` to the lucide import (alphabetical position, before `X`):

```tsx
import {
  Archive,
  ArchiveRestore,
  Check,
  FileText,
  GripVertical,
  Pencil,
  Plus,
  Search,
  Trash2,
  X,
} from "lucide-react";
```

Extend the view type and add the status helper below it:

```tsx
type View = "active" | "archive" | "search" | "deleted";
type TaskStatus = "active" | "archived" | "deleted";

function statusOf(task: Task): TaskStatus {
  return task.deletedAt ? "deleted" : task.completedAt ? "archived" : "active";
}
```

Add the deleted list next to `active`/`archived` and thread it through `visibleTasks` and the `selected` lookup:

```tsx
  const active = data?.activeTasks ?? [];
  const archived = data?.archivedTasks ?? [];
  const deleted = data?.deletedTasks ?? [];
  const visibleTasks = useMemo(() => {
    const source =
      view === "archive"
        ? archived
        : view === "deleted"
          ? deleted
          : view === "search"
            ? results
            : active;
    return source.filter((task) => selectedProjects.includes(task.projectId));
  }, [active, archived, deleted, results, selectedProjects, view]);
  const selected =
    [...active, ...archived, ...deleted].find(
      (task) => task.id === selectedId,
    ) ??
    visibleTasks[0] ??
    null;
```

In the keyboard handler, the Enter-to-edit guard must also exclude the read-only trash view — change `view !== "archive"` to:

```tsx
        view !== "archive" &&
        view !== "deleted"
```

- [ ] **Step 2: Add the trash state helpers**

Add after the `restore` function in `App`:

```tsx
  async function moveToTrash(task: Task) {
    try {
      const trashed = await api.deleteTask(task.id);
      setData(
        (current) =>
          current && {
            ...current,
            activeTasks: current.activeTasks.filter(
              (item) => item.id !== task.id,
            ),
            archivedTasks: current.archivedTasks.filter(
              (item) => item.id !== task.id,
            ),
            deletedTasks: [trashed, ...current.deletedTasks],
          },
      );
      setSelectedId(null);
    } catch (reason) {
      showError(reason);
    }
  }
  async function undelete(task: Task, to: "stack" | "archive") {
    try {
      const undeleted = await api.undeleteTask(task.id, to);
      setData(
        (current) =>
          current && {
            ...current,
            deletedTasks: current.deletedTasks.filter(
              (item) => item.id !== task.id,
            ),
            activeTasks:
              to === "stack"
                ? [undeleted, ...current.activeTasks]
                : current.activeTasks,
            archivedTasks:
              to === "archive"
                ? [undeleted, ...current.archivedTasks]
                : current.archivedTasks,
          },
      );
    } catch (reason) {
      showError(reason);
    }
  }
```

- [ ] **Step 3: Add the sidebar item and pane heading**

Add a Deleted nav button directly after the Archive button in the sidebar:

```tsx
          <button
            type="button"
            className={view === "deleted" ? "nav-item active" : "nav-item"}
            onClick={() => {
              setView("deleted");
              setQuery("");
            }}
          >
            <Trash2 size={16} />
            Deleted <span>{deleted.length}</span>
          </button>
```

Extend the pane heading's eyebrow chain — the full replacement expression is:

```tsx
                {view === "archive"
                  ? "Completed work"
                  : view === "deleted"
                    ? "Removed work"
                    : view === "search"
                      ? "Search results"
                      : selectedProjects.length === data.projects.length
                        ? "Your ordered stack"
                        : selectedProjects.length === 0
                          ? "No projects selected"
                          : selectedProjects.length === 1
                            ? data.projects.find(
                                (project) =>
                                  project.id === selectedProjects[0],
                              )?.name
                            : `${selectedProjects.length} projects`}
```

and the `<h1>` chain to:

```tsx
                {view === "archive"
                  ? "Archive"
                  : view === "deleted"
                    ? "Deleted"
                    : view === "search"
                      ? `Results for “${query}”`
                      : "What’s next"}
```

- [ ] **Step 4: Generalize `TaskEditor` from `archived` to `status`**

Change the `TaskEditor` call site:

```tsx
            <TaskEditor
              key={selected.id}
              task={selected}
              projects={data.projects}
              status={statusOf(selected)}
              onUpdate={updateTask}
              onComplete={complete}
              onRestore={restore}
              titleRef={titleRef}
            />
```

In `TaskEditor`, replace the `archived: boolean` prop with `status: TaskStatus` (update both the destructuring and the type annotation). Replace the meta line:

```tsx
        <span>
          {status === "deleted"
            ? `Deleted ${formatDate(task.deletedAt ?? task.modifiedAt)}`
            : status === "archived"
              ? `Completed ${formatDate(task.completedAt ?? task.modifiedAt)}`
              : `Updated ${formatDate(task.modifiedAt)}`}
        </span>
```

Replace the button ternary so only archived tasks get Restore and only active tasks get Complete (deleted tasks get no button — undelete arrives with drag in Task 7):

```tsx
        {status === "archived" ? (
          <button
            type="button"
            className="complete-text"
            onClick={() => onRestore(task)}
          >
            <ArchiveRestore size={15} />
            Restore to stack
          </button>
        ) : status === "active" ? (
          <button
            type="button"
            className="complete-text"
            onClick={() => onComplete(task)}
          >
            <Check size={15} />
            Complete
          </button>
        ) : null}
```

Change the read-only condition from `archived ? (...) : (...)` to `status !== "active" ? (...) : (...)` — the existing read-only branch (title, project tag, `ReadOnlySection`s) serves deleted tasks unchanged.

- [ ] **Step 5: Verify and inspect**

Run (from `frontend/`): `npm run check`
Expected: PASS. (`moveToTrash`/`undelete` are not referenced yet; if biome flags them as unused, suppress nothing — Task 7 wires them next, so if the gate fails on unused-symbol lint, proceed to Task 7's Step 1 before committing and commit both together with this task's message.)

- [ ] **Step 6: Format and commit**

```powershell
npm run fmt
git add -A && git commit -m "Trash view lists deleted tasks read-only"
```

---

### Task 7: Frontend — sidebar drop targets and lifted drag state

**Files:**

- Modify: `frontend/src/App.tsx`, `frontend/src/styles.css`

**Interfaces:**

- Consumes: `statusOf`, `moveToTrash`, `undelete` (Task 6); `complete`, `restore` (existing).
- Produces: `TaskList` props change to `{ tasks, selectedId, projects, draggable, canReorder, dragged, onSelect, onComplete, onReorder, onDragStart, onDragEnd }`; CSS class `nav-item drop-ready`. Task 8 modifies this `TaskList` signature again (drops `onComplete`).

- [ ] **Step 1: Lift drag state into `App` and define drop-target helpers**

In `App`, add state next to the other `useState` calls:

```tsx
  const [dragged, setDragged] = useState<Task | null>(null);
  const [dropHover, setDropHover] = useState<
    "stack" | "archive" | "trash" | null
  >(null);
```

Add these helpers after the `undelete` function (`DropTarget` is a type alias placed next to `TaskStatus` at module level):

```tsx
type DropTarget = "stack" | "archive" | "trash";
```

```tsx
  function dropAction(target: DropTarget, task: Task): (() => void) | null {
    const status = statusOf(task);
    if (target === "archive" && status === "active")
      return () => complete(task);
    if (target === "archive" && status === "deleted")
      return () => undelete(task, "archive");
    if (target === "trash" && status !== "deleted")
      return () => moveToTrash(task);
    if (target === "stack" && status === "archived")
      return () => restore(task);
    if (target === "stack" && status === "deleted")
      return () => undelete(task, "stack");
    return null;
  }
  function dropTargetProps(target: DropTarget) {
    return {
      onDragOver: (event: DragEvent<HTMLButtonElement>) => {
        if (dragged && dropAction(target, dragged)) {
          event.preventDefault();
          setDropHover(target);
        }
      },
      onDragLeave: () =>
        setDropHover((current) => (current === target ? null : current)),
      onDrop: (event: DragEvent<HTMLButtonElement>) => {
        event.preventDefault();
        if (dragged) dropAction(target, dragged)?.();
        setDragged(null);
        setDropHover(null);
      },
    };
  }
  const endDrag = () => {
    setDragged(null);
    setDropHover(null);
  };
```

(The react `DragEvent` type is already imported at the top of the file.)

- [ ] **Step 2: Wire the three sidebar targets**

**All tasks** button — spread the props and extend the class:

```tsx
          <button
            type="button"
            className={
              dropHover === "stack" ? "nav-item drop-ready" : "nav-item"
            }
            onClick={() => {
              setView("active");
              changeSelection(data.projects.map((project) => project.id));
            }}
            {...dropTargetProps("stack")}
          >
            <FileText size={16} />
            All tasks <span>{active.length}</span>
          </button>
```

**Archive** button — combine active and drop-ready classes:

```tsx
          <button
            type="button"
            className={`nav-item${view === "archive" ? " active" : ""}${
              dropHover === "archive" ? " drop-ready" : ""
            }`}
            onClick={() => {
              setView("archive");
              setQuery("");
            }}
            {...dropTargetProps("archive")}
          >
            <Archive size={16} />
            Archive <span>{archived.length}</span>
          </button>
```

**Deleted** button (added in Task 6) — same treatment:

```tsx
          <button
            type="button"
            className={`nav-item${view === "deleted" ? " active" : ""}${
              dropHover === "trash" ? " drop-ready" : ""
            }`}
            onClick={() => {
              setView("deleted");
              setQuery("");
            }}
            {...dropTargetProps("trash")}
          >
            <Trash2 size={16} />
            Deleted <span>{deleted.length}</span>
          </button>
```

- [ ] **Step 3: Rework `TaskList` to use the lifted state**

Change the `TaskList` call site in `App`:

```tsx
          <TaskList
            tasks={visibleTasks}
            selectedId={selected?.id ?? null}
            projects={data.projects}
            draggable={view !== "search"}
            canReorder={view === "active"}
            dragged={dragged}
            onSelect={(task) => setSelectedId(task.id)}
            onComplete={complete}
            onReorder={reorder}
            onDragStart={setDragged}
            onDragEnd={endDrag}
          />
```

Replace `TaskList`'s signature and drag handling (the local `dragged` state is deleted; `draggable` and `canReorder` are now separate concerns):

```tsx
function TaskList({
  tasks,
  selectedId,
  projects,
  draggable,
  canReorder,
  dragged,
  onSelect,
  onComplete,
  onReorder,
  onDragStart,
  onDragEnd,
}: {
  tasks: Task[];
  selectedId: string | null;
  projects: Project[];
  draggable: boolean;
  canReorder: boolean;
  dragged: Task | null;
  onSelect: (task: Task) => void;
  onComplete: (task: Task) => void;
  onReorder: (task: Task, target: Task, after: boolean) => void;
  onDragStart: (task: Task) => void;
  onDragEnd: () => void;
}) {
  if (!tasks.length)
    return (
      <div className="empty-list">
        <FileText size={24} />
        <h2>No tasks here</h2>
        <p>Capture the next useful piece of work.</p>
      </div>
    );
  const drop = (event: DragEvent, target: Task) => {
    event.preventDefault();
    const bounds = event.currentTarget.getBoundingClientRect();
    const after = event.clientY > bounds.top + bounds.height / 2;
    if (dragged && dragged.id !== target.id) onReorder(dragged, target, after);
    onDragEnd();
  };
```

and the row attributes:

```tsx
          <li
            key={task.id}
            className={
              selectedId === task.id ? "task-row selected" : "task-row"
            }
            style={projectStyle(project?.color)}
            draggable={draggable}
            onDragStart={() => onDragStart(task)}
            onDragEnd={onDragEnd}
            onDragOver={(event) => {
              if (canReorder) event.preventDefault();
            }}
            onDrop={(event) => {
              if (canReorder) drop(event, task);
            }}
          >
```

Show the grip whenever the row is draggable (was `canReorder`):

```tsx
              {draggable && <GripVertical className="grab" size={17} />}
```

The ✓ complete button keeps its `{canReorder && ...}` guard for now — Task 8 removes it entirely.

- [ ] **Step 4: Add the drop-ready style**

In `frontend/src/styles.css`, after the `.nav-item.active` rule (~line 187):

```css
.nav-item.drop-ready {
  background: rgba(0, 82, 255, 0.22);
  color: var(--text-primary);
  outline: 1px dashed rgba(0, 82, 255, 0.7);
  outline-offset: -1px;
}
```

- [ ] **Step 5: Verify the gate and smoke-test the drags**

Run (from `frontend/`): `npm run check`
Expected: PASS.

Start the app (`scripts/start.ps1` from the repo root) and verify by hand:

1. Dragging a stack row over Archive and Deleted highlights them; over All tasks and projects it does not.
2. Dropping on Archive completes the task; dropping on Deleted trashes it; counts update.
3. In the archive view, dragging a row onto All tasks restores it to the stack top; onto Deleted trashes it.
4. In the trash view, dragging a row onto All tasks makes it active on top; onto Archive files it as completed.
5. Row-to-row reordering in the stack still works and only in the stack.

- [ ] **Step 6: Format and commit**

```powershell
npm run fmt
git add -A && git commit -m "Sidebar items accept task drops for archive, trash, and restore"
```

---

### Task 8: Frontend — remove the archive/restore buttons

**Files:**

- Modify: `frontend/src/App.tsx`, `frontend/src/styles.css`

**Interfaces:**

- Consumes: working drop targets from Task 7 (the buttons' functionality must already be reachable by drag before removal).
- Produces: `TaskList` props lose `onComplete`; `TaskEditor` props lose `onComplete` and `onRestore`. The Delete-key shortcut (in `App`'s keydown handler) is untouched and still calls `complete`.

- [ ] **Step 1: Remove the ✓ button from task rows**

In `TaskList`, delete the block:

```tsx
            {canReorder && (
              <button
                type="button"
                className="complete"
                onClick={(event) => {
                  event.stopPropagation();
                  onComplete(task);
                }}
                aria-label={`Complete ${task.title}`}
              >
                <Check size={16} />
              </button>
            )}
```

Remove `onComplete` from `TaskList`'s props (destructuring, type annotation, and the `onComplete={complete}` line at the call site in `App`).

- [ ] **Step 2: Remove the editor buttons**

In `TaskEditor`, delete the entire `{status === "archived" ? (...) : status === "active" ? (...) : null}` button block from `editor-meta`, leaving only the date `<span>`. Remove the `onComplete` and `onRestore` props (destructuring, type annotation, and the `onComplete={complete}` / `onRestore={restore}` lines at the call site).

Remove `ArchiveRestore` from the lucide import (`Check` stays — the empty-detail placeholder uses it).

- [ ] **Step 3: Remove dead CSS**

In `frontend/src/styles.css`, delete the `.complete`, `.complete:hover`, and `.complete-text` rules (~lines 352–366 and 384–392).

- [ ] **Step 4: Verify the gate and confirm nothing is orphaned**

Run (from `frontend/`): `npm run check`
Expected: PASS — biome's unused-variable checks confirm no orphaned handlers. `complete` and `restore` in `App` must still be referenced (Delete key / `dropAction`); if biome reports either unused, the drop wiring from Task 7 regressed — stop and fix.

In the running app: rows show no ✓ button; the editor shows only the date line above the title; Delete key still completes the selected task in the active view.

- [ ] **Step 5: Format and commit**

```powershell
npm run fmt
git add -A && git commit -m "Archive and restore happen by drag instead of buttons"
```

---

### Task 9: Documentation reconciliation

**Files:**

- Modify: `docs/RequirementSpecification.md` (sections "Delete / Complete Task" ~line 104, "Archive" ~line 183, keyboard table ~line 238), `docs/DecisionLog.md` (append), `README.md` (feature list ~line 19)

**Interfaces:** none (prose only).

- [ ] **Step 1: Update `docs/RequirementSpecification.md`**

Read the current "Delete / Complete Task" (~line 104) and "Archive" (~line 183) sections first, then rework them so they state exactly these behaviors (keep the surrounding document voice; these are the required facts, not verbatim text to paste):

- Completing: drag a task from the stack onto the **Archive** sidebar item, or press the Delete key on the selected task in the active view. Completed tasks move to the archive.
- Deleting: drag a task (active or archived) onto the **Deleted** sidebar item. Deleted tasks keep all fields and timestamps but are excluded from search, reorder, and every mutation except undelete. There is no permanent deletion.
- Restoring: drag an archived task onto **All tasks**; it returns to the top of the active stack and its completed timestamp is cleared.
- Undeleting (new "Trash" subsection under "Archive"): the drop target chooses the destination — **All tasks** makes the task active at the top of the stack; **Archive** files it as completed with a *fresh* completed timestamp (the original is not preserved, because deleting clears it).
- The sidebar view list (~line 222) gains a "Deleted" entry alongside "Archive".
- The keyboard table row `Delete | Complete selected task` stays unchanged.
- Remove any remaining sentences describing complete/restore *buttons*.

- [ ] **Step 2: Append to `docs/DecisionLog.md`**

Add a new dated entry at the end (do not rewrite old entries):

```markdown
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
```

- [ ] **Step 3: Update `README.md`**

Extend the feature list around line 19: change "Completion to a read-only searchable archive" to mention drag-to-archive, and add a bullet for the trash, e.g. "Drag tasks onto Archive or Deleted in the sidebar to complete or trash them; drag them back out to restore. Deleted tasks are kept out of search but never destroyed."

- [ ] **Step 4: Commit**

```powershell
git add -A && git commit -m "Document drag-and-drop archiving and the trash state"
```

---

### Task 10: Final verification

**Files:** none (verification only).

- [ ] **Step 1: Full backend gate**

Run (from `backend/`): `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all tests pass, no clippy warnings, no formatting diffs.

- [ ] **Step 2: Full frontend gate**

Run (from `frontend/`): `npm run check`
Expected: PASS.

- [ ] **Step 3: Manual verification pass (spec checklist)**

Start the app with `scripts/start.ps1` and walk the spec's manual list:

1. Drag a task from the stack onto Archive — completes; Archive highlights during hover; counts update.
2. Drag a task onto Deleted — moves to trash; repeat from the archive view.
3. Drag from the archive onto All tasks — returns to the top of the stack, project intact.
4. Drag from the trash onto All tasks and onto Archive — lands active on top, respectively archived with a fresh completion date.
5. Row reordering in the stack still works; dropping a row on a project or empty space does nothing.
6. Invalid targets do not highlight (e.g., active task over All tasks; anything over a project).
7. Delete key still completes in the active view; search finds active and archived but not deleted tasks.
8. Restart the backend (Ctrl+C, rerun `scripts/start.ps1`) — trashed tasks are still in the trash (migration + persistence).

- [ ] **Step 4: Fix anything found, then finish**

If a manual step fails, use superpowers:systematic-debugging before changing code. When all pass, the branch is ready for superpowers:finishing-a-development-branch.
