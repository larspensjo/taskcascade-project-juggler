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
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
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
    assert!(matches!(task.get("deletedAt"), Some(Value::Null)));

    assert!(bootstrap(&app).await["deletedTasks"]
        .as_array()
        .expect("deletedTasks array")
        .is_empty());
}

#[tokio::test]
async fn delete_moves_active_task_to_trash() {
    let app = setup().await;
    let task_id = create_task(&app, &create_project(&app).await, "A").await;
    let (status, deleted) = send(&app, "POST", &format!("/api/tasks/{task_id}/delete"), None).await;
    assert_eq!(status, StatusCode::OK, "delete should succeed: {deleted}");
    assert!(deleted["deletedAt"].is_string());
    assert!(deleted["completedAt"].is_null());
    let boot = bootstrap(&app).await;
    assert!(boot["activeTasks"]
        .as_array()
        .expect("activeTasks")
        .is_empty());
    assert_eq!(
        boot["deletedTasks"].as_array().expect("deletedTasks").len(),
        1
    );
}

#[tokio::test]
async fn delete_clears_completed_at_on_archived_task() {
    let app = setup().await;
    let task_id = create_task(&app, &create_project(&app).await, "A").await;
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/tasks/{task_id}/complete"),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, deleted) = send(&app, "POST", &format!("/api/tasks/{task_id}/delete"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(deleted["deletedAt"].is_string());
    assert!(deleted["completedAt"].is_null());
    let boot = bootstrap(&app).await;
    assert!(boot["archivedTasks"]
        .as_array()
        .expect("archivedTasks")
        .is_empty());
    assert_eq!(
        boot["deletedTasks"].as_array().expect("deletedTasks").len(),
        1
    );
}

#[tokio::test]
async fn delete_rejects_deleted_and_unknown_tasks() {
    let app = setup().await;
    let task_id = create_task(&app, &create_project(&app).await, "A").await;
    assert_eq!(
        send(&app, "POST", &format!("/api/tasks/{task_id}/delete"), None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, "POST", &format!("/api/tasks/{task_id}/delete"), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/tasks/00000000-0000-0000-0000-000000000000/delete",
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn undelete_to_stack_restores_to_top() {
    let app = setup().await;
    let project_id = create_project(&app).await;
    let b_id = create_task(&app, &project_id, "B").await;
    let a_id = create_task(&app, &project_id, "A").await;
    assert_eq!(
        send(&app, "POST", &format!("/api/tasks/{b_id}/delete"), None)
            .await
            .0,
        StatusCode::OK
    );
    let (status, task) = send(
        &app,
        "POST",
        &format!("/api/tasks/{b_id}/undelete"),
        Some(json!({ "to": "stack" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task["position"], 0);
    assert!(task["deletedAt"].is_null());
    assert!(task["completedAt"].is_null());
    let active = bootstrap(&app).await["activeTasks"]
        .as_array()
        .expect("activeTasks")
        .clone();
    assert_eq!(active.len(), 2);
    assert_eq!(active[0]["id"], b_id);
    assert_eq!(active[1]["id"], a_id);
}

#[tokio::test]
async fn undelete_to_archive_stamps_fresh_completed_at() {
    let app = setup().await;
    let task_id = create_task(&app, &create_project(&app).await, "A").await;
    assert_eq!(
        send(&app, "POST", &format!("/api/tasks/{task_id}/delete"), None)
            .await
            .0,
        StatusCode::OK
    );
    let (status, task) = send(
        &app,
        "POST",
        &format!("/api/tasks/{task_id}/undelete"),
        Some(json!({ "to": "archive" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(task["completedAt"].is_string());
    assert!(task["deletedAt"].is_null());
    let boot = bootstrap(&app).await;
    assert_eq!(
        boot["archivedTasks"]
            .as_array()
            .expect("archivedTasks")
            .len(),
        1
    );
    assert!(boot["deletedTasks"]
        .as_array()
        .expect("deletedTasks")
        .is_empty());
}

#[tokio::test]
async fn undelete_rejects_bad_destination_and_non_deleted_tasks() {
    let app = setup().await;
    let project_id = create_project(&app).await;
    let deleted_id = create_task(&app, &project_id, "B").await;
    let active_id = create_task(&app, &project_id, "A").await;
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/tasks/{deleted_id}/delete"),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/tasks/{deleted_id}/undelete"),
            Some(json!({ "to": "somewhere" }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/tasks/{active_id}/undelete"),
            Some(json!({ "to": "stack" }))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let active = bootstrap(&app).await["activeTasks"]
        .as_array()
        .expect("activeTasks")
        .clone();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0]["id"], active_id);
    assert_eq!(active[0]["position"], 0);
}

#[tokio::test]
async fn deleted_tasks_are_excluded_from_mutations_search_and_reorder() {
    let app = setup().await;
    let project_id = create_project(&app).await;
    let deleted_id = create_task(&app, &project_id, "B needle").await;
    let active_id = create_task(&app, &project_id, "A").await;
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/tasks/{deleted_id}/delete"),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(send(&app, "PUT", &format!("/api/tasks/{deleted_id}"), Some(json!({ "title": "B2", "projectId": project_id, "description": "", "scratchpad": "" }))).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/tasks/{deleted_id}/complete"),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/tasks/{deleted_id}/restore"),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, results) = send(&app, "GET", "/api/search?q=needle", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(results.as_array().expect("search results").is_empty());
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/tasks/{active_id}/reorder"),
            Some(json!({ "targetTaskId": deleted_id, "after": true }))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
