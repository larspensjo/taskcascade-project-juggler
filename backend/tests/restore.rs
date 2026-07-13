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

fn assert_stack_b_on_top(boot: &Value, a_id: &str, b_id: &str) {
    let active = boot["activeTasks"].as_array().expect("activeTasks array");
    assert_eq!(active.len(), 2);
    assert_eq!(active[0]["id"], *b_id);
    assert_eq!(active[0]["position"], 0);
    assert_eq!(active[1]["id"], *a_id);
    assert_eq!(active[1]["position"], 1);
    let archived = boot["archivedTasks"]
        .as_array()
        .expect("archivedTasks array");
    assert!(archived.is_empty(), "archive should be empty");
}

#[tokio::test]
async fn restore_returns_archived_task_to_top_of_stack() {
    let app = setup().await;

    // Create a project and two tasks: B first, then A, so A sits on top.
    let (status, project) = send(
        &app,
        "POST",
        "/api/projects",
        Some(json!({ "name": "Engine" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let project_id = project["id"].as_str().expect("project id").to_owned();

    let (status, task_b) = send(
        &app,
        "POST",
        "/api/tasks",
        Some(json!({ "title": "B", "projectId": project_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let b_id = task_b["id"].as_str().expect("task id").to_owned();

    let (status, task_a) = send(
        &app,
        "POST",
        "/api/tasks",
        Some(json!({ "title": "A", "projectId": project_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let a_id = task_a["id"].as_str().expect("task id").to_owned();

    // Complete B: it leaves the active list and enters the archive.
    let (status, completed) =
        send(&app, "POST", &format!("/api/tasks/{b_id}/complete"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(completed["completedAt"].is_string());

    // Restore B: back on top, completedAt cleared, A bumped to position 1.
    let (status, restored) = send(&app, "POST", &format!("/api/tasks/{b_id}/restore"), None).await;
    assert_eq!(status, StatusCode::OK, "restore should succeed: {restored}");
    assert_eq!(restored["id"], *b_id);
    assert_eq!(restored["position"], 0);
    assert!(restored["completedAt"].is_null());

    let boot = bootstrap(&app).await;
    assert_stack_b_on_top(&boot, &a_id, &b_id);

    // Restoring B again is a 404 (already active), and the rejected restore
    // must not shift the stack — this verifies the transaction rollback.
    let (status, _) = send(&app, "POST", &format!("/api/tasks/{b_id}/restore"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let boot = bootstrap(&app).await;
    assert_stack_b_on_top(&boot, &a_id, &b_id);

    // Restoring a nonexistent id is also a 404 that leaves the stack alone.
    let (status, _) = send(
        &app,
        "POST",
        "/api/tasks/00000000-0000-0000-0000-000000000000/restore",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let boot = bootstrap(&app).await;
    assert_stack_b_on_top(&boot, &a_id, &b_id);
}
