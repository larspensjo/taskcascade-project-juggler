use taskcascade_backend::app::{summarize_database, AppState, OpenError, OpenMode};

#[tokio::test]
async fn existing_only_refuses_a_missing_file_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.sqlite");
    let error = match AppState::open(&path, OpenMode::ExistingOnly).await {
        Err(error) => error,
        Ok(_) => panic!("existing-only must refuse a missing database"),
    };
    assert_eq!(
        error.to_string(),
        format!("No task database at {}.", path.display())
    );
    assert!(matches!(error, OpenError::Missing(p) if p == path));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

async fn assert_migrated_and_seeded(state: &AppState, path: &std::path::Path) {
    let mut connection = state.acquire_for_tests().await;
    let (name, color): (String, Option<String>) =
        sqlx::query_as("SELECT name, color FROM projects")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(name, "Personal");
    assert!(color.is_some());
    drop(connection);
    let summary = summarize_database(path).await.unwrap();
    assert_eq!(summary.projects, 1);
    assert_eq!(summary.active_tasks, 0);
    assert_eq!(summary.integrity, "ok");
    assert_eq!(
        summary.schema_version,
        sqlx::migrate!("./migrations")
            .migrations
            .iter()
            .map(|m| m.version)
            .max()
    );
}

#[tokio::test]
async fn create_if_missing_creates_migrates_and_seeds_personal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.sqlite");
    let state = AppState::open(&path, OpenMode::CreateIfMissing)
        .await
        .unwrap();
    assert!(path.is_file());
    assert_migrated_and_seeded(&state, &path).await;
    state.close().await;
    let reopened = AppState::open(&path, OpenMode::CreateIfMissing)
        .await
        .unwrap();
    assert_migrated_and_seeded(&reopened, &path).await;
    reopened.close().await;
}

#[tokio::test]
async fn existing_only_opens_and_migrates_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.sqlite");
    std::fs::File::create(&path).unwrap();
    let state = AppState::open(&path, OpenMode::ExistingOnly).await.unwrap();
    assert_migrated_and_seeded(&state, &path).await;
    state.close().await;
}

#[tokio::test]
async fn close_releases_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.sqlite");
    let state = AppState::open(&path, OpenMode::CreateIfMissing)
        .await
        .unwrap();
    state.close().await;
    std::fs::rename(&path, dir.path().join("closed.sqlite")).unwrap();
}

#[tokio::test]
async fn close_waits_for_checked_out_connection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.sqlite");
    let state = AppState::open(&path, OpenMode::CreateIfMissing)
        .await
        .unwrap();
    let held = state.acquire_for_tests().await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), state.close())
            .await
            .is_err()
    );
    drop(held);
    state.close().await;
}
