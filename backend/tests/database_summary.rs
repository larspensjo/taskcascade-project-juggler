use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use taskcascade_backend::{
    app::{router, summarize_database, AppState, SummaryError},
    test_support::{build_fixture, FixtureKind},
};
use tower::ServiceExt;

async fn send(app: &Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn summary_counts_tasks_by_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sqlite");
    let app = router(
        AppState::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap(),
    );
    let (_, bootstrap) = send(&app, "GET", "/api/bootstrap", None).await;
    let project = &bootstrap["projects"][0]["id"];
    let mut ids = Vec::new();
    for title in ["One", "Two", "Three", "Four"] {
        let (status, task) = send(
            &app,
            "POST",
            "/api/tasks",
            Some(json!({"title": title, "projectId": project})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        ids.push(task["id"].as_str().unwrap().to_owned());
    }
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/tasks/{}/complete", ids[0]),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, "POST", &format!("/api/tasks/{}/delete", ids[1]), None)
            .await
            .0,
        StatusCode::OK
    );
    let summary = summarize_database(&path).await.unwrap();
    assert_eq!(summary.integrity, "ok");
    assert_eq!(summary.projects, 1);
    assert_eq!(
        (
            summary.active_tasks,
            summary.archived_tasks,
            summary.deleted_tasks
        ),
        (2, 1, 1)
    );
    let latest_migration = sqlx::migrate!("./migrations")
        .migrations
        .iter()
        .map(|migration| migration.version)
        .max();
    assert_eq!(summary.schema_version, latest_migration);
    assert!(summary.newest_modified_at.is_some());
}

#[tokio::test]
async fn summary_of_a_missing_file_is_an_error_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.sqlite");
    assert!(matches!(summarize_database(&path).await, Err(SummaryError::Missing(p)) if p == path));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn summary_reads_committed_wal_content() {
    let source = tempfile::tempdir().unwrap();
    let (expected, wal_connection) = build_fixture(source.path(), FixtureKind::Wal).await;
    let complete = tempfile::tempdir().unwrap();
    for suffix in ["", "-wal", "-shm"] {
        let name = format!("taskcascade.sqlite{suffix}");
        std::fs::copy(source.path().join(&name), complete.path().join(&name)).unwrap();
    }
    let main_only = tempfile::tempdir().unwrap();
    std::fs::copy(
        source.path().join("taskcascade.sqlite"),
        main_only.path().join("taskcascade.sqlite"),
    )
    .unwrap();
    // Copies are complete; closing now may checkpoint the source harmlessly.
    sqlx::Connection::close(wal_connection.unwrap())
        .await
        .unwrap();
    source.close().expect("fixture directory must be removable");
    assert_eq!(
        summarize_database(&complete.path().join("taskcascade.sqlite"))
            .await
            .unwrap(),
        expected
    );
    let without_wal = summarize_database(&main_only.path().join("taskcascade.sqlite"))
        .await
        .unwrap();
    assert_ne!(without_wal, expected);
    assert_eq!(without_wal.active_tasks + 1, expected.active_tasks);
}
