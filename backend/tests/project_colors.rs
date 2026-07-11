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

fn assert_hex_color(value: &Value) -> String {
    let color = value.as_str().expect("color should be a string");
    assert_eq!(color.len(), 7, "color should be #rrggbb: {color}");
    assert!(color.starts_with('#'), "color should start with #: {color}");
    assert!(
        color[1..].chars().all(|c| c.is_ascii_hexdigit()),
        "color should be hex digits: {color}"
    );
    color.to_owned()
}

#[tokio::test]
async fn seeded_project_has_palette_color() {
    let app = setup().await;
    let (status, boot) = send(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    let projects = boot["projects"].as_array().expect("projects array");
    assert_eq!(projects.len(), 1);
    assert_hex_color(&projects[0]["color"]);
}

#[tokio::test]
async fn new_projects_get_distinct_palette_colors() {
    let app = setup().await;
    let mut colors = Vec::new();
    let (_, boot) = send(&app, "GET", "/api/bootstrap", None).await;
    colors.push(assert_hex_color(&boot["projects"][0]["color"]));
    for name in ["Engine", "UI", "Infra", "Docs"] {
        let (status, project) =
            send(&app, "POST", "/api/projects", Some(json!({ "name": name }))).await;
        assert_eq!(status, StatusCode::OK);
        colors.push(assert_hex_color(&project["color"]));
    }
    let unique: std::collections::HashSet<&String> = colors.iter().collect();
    assert_eq!(
        unique.len(),
        colors.len(),
        "auto-assigned colors should be distinct: {colors:?}"
    );
}

#[tokio::test]
async fn create_project_accepts_explicit_color() {
    let app = setup().await;
    let (status, project) = send(
        &app,
        "POST",
        "/api/projects",
        Some(json!({ "name": "Engine", "color": "#68217a" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(project["color"], "#68217a");

    // Persisted, not just echoed.
    let (_, boot) = send(&app, "GET", "/api/bootstrap", None).await;
    let projects = boot["projects"].as_array().expect("projects array");
    let engine = projects
        .iter()
        .find(|p| p["name"] == "Engine")
        .expect("Engine project");
    assert_eq!(engine["color"], "#68217a");
}

#[tokio::test]
async fn create_project_rejects_invalid_color() {
    let app = setup().await;
    for bad in ["red", "#12345", "#12345g", "123456", "#1234567"] {
        let (status, _) = send(
            &app,
            "POST",
            "/api/projects",
            Some(json!({ "name": format!("P{bad}"), "color": bad })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "should reject {bad}");
    }
}

#[tokio::test]
async fn update_project_changes_name_and_color() {
    let app = setup().await;
    let (status, project) = send(
        &app,
        "POST",
        "/api/projects",
        Some(json!({ "name": "Engine" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = project["id"].as_str().expect("project id").to_owned();

    let (status, updated) = send(
        &app,
        "PUT",
        &format!("/api/projects/{id}"),
        Some(json!({ "name": "Engine Core", "color": "#68217a" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update should succeed: {updated}");
    assert_eq!(updated["id"], *id);
    assert_eq!(updated["name"], "Engine Core");
    assert_eq!(updated["color"], "#68217a");

    let (_, boot) = send(&app, "GET", "/api/bootstrap", None).await;
    let projects = boot["projects"].as_array().expect("projects array");
    let engine = projects
        .iter()
        .find(|p| p["id"] == *id)
        .expect("updated project");
    assert_eq!(engine["name"], "Engine Core");
    assert_eq!(engine["color"], "#68217a");
}

#[tokio::test]
async fn update_project_rejects_invalid_input() {
    let app = setup().await;
    let (_, project) = send(
        &app,
        "POST",
        "/api/projects",
        Some(json!({ "name": "Engine" })),
    )
    .await;
    let id = project["id"].as_str().expect("project id").to_owned();

    let (status, _) = send(
        &app,
        "PUT",
        &format!("/api/projects/{id}"),
        Some(json!({ "name": "Engine", "color": "blue" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send(
        &app,
        "PUT",
        &format!("/api/projects/{id}"),
        Some(json!({ "name": "", "color": "#68217a" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send(
        &app,
        "PUT",
        "/api/projects/00000000-0000-0000-0000-000000000000",
        Some(json!({ "name": "Ghost", "color": "#68217a" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
