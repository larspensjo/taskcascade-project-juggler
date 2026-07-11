use std::{str::FromStr, sync::Arc};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    FromRow, SqlitePool,
};
use tower_http::{cors::CorsLayer, services::ServeDir};
use uuid::Uuid;

use crate::domain::relocate;

#[derive(Clone)]
pub struct AppState(Arc<SqlitePool>);

impl AppState {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        let has_projects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects")
            .fetch_one(&pool)
            .await?;
        if has_projects == 0 {
            sqlx::query("INSERT INTO projects (id, name, created_at) VALUES (?, 'Personal', ?)")
                .bind(Uuid::new_v4().to_string())
                .bind(now())
                .execute(&pool)
                .await?;
        }
        assign_missing_colors(&pool).await?;
        Ok(Self(Arc::new(pool)))
    }
}

/// Dark-theme-friendly defaults; projects keep working if the user replaces
/// them with arbitrary hex values.
const PALETTE: [&str; 8] = [
    "#e8833a", "#9a6bff", "#2bb8a3", "#e05c78", "#5aa9e6", "#a8b545", "#d9a03c", "#c65bc9",
];

async fn assign_missing_colors(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let missing: Vec<String> =
        sqlx::query_scalar("SELECT id FROM projects WHERE color IS NULL ORDER BY created_at, id")
            .fetch_all(pool)
            .await?;
    if missing.is_empty() {
        return Ok(());
    }
    let mut used: Vec<String> =
        sqlx::query_scalar("SELECT color FROM projects WHERE color IS NOT NULL")
            .fetch_all(pool)
            .await?;
    for id in missing {
        let color = next_free_color(&used);
        sqlx::query("UPDATE projects SET color = ? WHERE id = ?")
            .bind(&color)
            .bind(&id)
            .execute(pool)
            .await?;
        used.push(color);
    }
    Ok(())
}

fn next_free_color(used: &[String]) -> String {
    PALETTE
        .iter()
        .find(|color| !used.iter().any(|candidate| candidate.eq_ignore_ascii_case(color)))
        .map_or_else(
            || PALETTE[used.len() % PALETTE.len()].to_owned(),
            |color| (*color).to_owned(),
        )
}

fn parse_color(value: &str) -> Result<String, ApiError> {
    let trimmed = value.trim();
    let is_hex = trimmed.len() == 7
        && trimmed.starts_with('#')
        && trimmed[1..].chars().all(|c| c.is_ascii_hexdigit());
    if is_hex {
        Ok(trimmed.to_ascii_lowercase())
    } else {
        Err(ApiError::bad_request(
            "Color must be a hex value like #4488cc.",
        ))
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/search", get(search))
        .route("/api/projects", post(create_project))
        .route("/api/projects/{id}", put(update_project))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/{id}", put(update_task))
        .route("/api/tasks/{id}/complete", post(complete_task))
        .route("/api/tasks/{id}/restore", post(restore_task))
        .route("/api/tasks/{id}/reorder", post(reorder_task))
        .route("/api/preferences/{key}", put(save_preference))
        .fallback_service(ServeDir::new("../frontend/dist"))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

#[derive(Serialize)]
struct Health {
    version: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
struct Project {
    id: String,
    name: String,
    color: String,
    created_at: String,
}

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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bootstrap {
    projects: Vec<Project>,
    active_tasks: Vec<Task>,
    archived_tasks: Vec<Task>,
    preferences: Vec<Preference>,
}

#[derive(Debug, Serialize, FromRow)]
struct Preference {
    key: String,
    value: String,
}

async fn bootstrap(State(state): State<AppState>) -> Result<Json<Bootstrap>, ApiError> {
    let projects = sqlx::query_as::<_, Project>(
        "SELECT id, name, color, created_at FROM projects ORDER BY name",
    )
    .fetch_all(state.0.as_ref())
    .await?;
    let active_tasks = fetch_tasks(&state.0, false).await?;
    let archived_tasks = fetch_tasks(&state.0, true).await?;
    let preferences = sqlx::query_as::<_, Preference>("SELECT key, value FROM preferences")
        .fetch_all(state.0.as_ref())
        .await?;
    Ok(Json(Bootstrap {
        projects,
        active_tasks,
        archived_tasks,
        preferences,
    }))
}

async fn fetch_tasks(pool: &SqlitePool, archived: bool) -> Result<Vec<Task>, sqlx::Error> {
    let predicate = if archived {
        "completed_at IS NOT NULL"
    } else {
        "completed_at IS NULL"
    };
    sqlx::query_as::<_, Task>(&format!(
        "SELECT id, title, description, scratchpad, project_id, position, created_at, modified_at, completed_at FROM tasks WHERE {predicate} ORDER BY {}",
        if archived { "completed_at DESC" } else { "position ASC" }
    )).fetch_all(pool).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProject {
    name: String,
    color: Option<String>,
}

async fn create_project(
    State(state): State<AppState>,
    Json(input): Json<CreateProject>,
) -> Result<Json<Project>, ApiError> {
    let name = required(&input.name, "Project name")?;
    let color = match &input.color {
        Some(value) => parse_color(value)?,
        None => {
            let used: Vec<String> =
                sqlx::query_scalar("SELECT color FROM projects WHERE color IS NOT NULL")
                    .fetch_all(state.0.as_ref())
                    .await?;
            next_free_color(&used)
        }
    };
    let project = Project {
        id: Uuid::new_v4().to_string(),
        name,
        color,
        created_at: now(),
    };
    sqlx::query("INSERT INTO projects (id, name, color, created_at) VALUES (?, ?, ?, ?)")
        .bind(&project.id)
        .bind(&project.name)
        .bind(&project.color)
        .bind(&project.created_at)
        .execute(state.0.as_ref())
        .await?;
    Ok(Json(project))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProject {
    name: String,
    color: String,
}

async fn update_project(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<UpdateProject>,
) -> Result<Json<Project>, ApiError> {
    let name = required(&input.name, "Project name")?;
    let color = parse_color(&input.color)?;
    let result = sqlx::query("UPDATE projects SET name = ?, color = ? WHERE id = ?")
        .bind(&name)
        .bind(&color)
        .bind(&id)
        .execute(state.0.as_ref())
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: "The project was not found.".into(),
        });
    }
    let project = sqlx::query_as::<_, Project>(
        "SELECT id, name, color, created_at FROM projects WHERE id = ?",
    )
    .bind(id)
    .fetch_one(state.0.as_ref())
    .await?;
    Ok(Json(project))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTask {
    title: String,
    project_id: String,
    description: Option<String>,
}

async fn create_task(
    State(state): State<AppState>,
    Json(input): Json<CreateTask>,
) -> Result<Json<Task>, ApiError> {
    let title = required(&input.title, "Title")?;
    let exists: Option<String> = sqlx::query_scalar("SELECT id FROM projects WHERE id = ?")
        .bind(&input.project_id)
        .fetch_optional(state.0.as_ref())
        .await?;
    if exists.is_none() {
        return Err(ApiError::bad_request("Choose a valid project."));
    }
    let timestamp = now();
    let task = Task {
        id: Uuid::new_v4().to_string(),
        title,
        description: input.description.unwrap_or_default(),
        scratchpad: String::new(),
        project_id: input.project_id,
        position: 0,
        created_at: timestamp.clone(),
        modified_at: timestamp,
        completed_at: None,
    };
    let mut tx = state.0.begin().await?;
    sqlx::query("UPDATE tasks SET position = position + 1 WHERE completed_at IS NULL")
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO tasks (id, title, description, scratchpad, project_id, position, created_at, modified_at) VALUES (?, ?, ?, ?, ?, 0, ?, ?)")
        .bind(&task.id).bind(&task.title).bind(&task.description).bind(&task.scratchpad).bind(&task.project_id).bind(&task.created_at).bind(&task.modified_at).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(task))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateTask {
    title: String,
    project_id: String,
    description: String,
    scratchpad: String,
}

async fn update_task(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<UpdateTask>,
) -> Result<Json<Task>, ApiError> {
    let title = required(&input.title, "Title")?;
    let project_exists: Option<String> = sqlx::query_scalar("SELECT id FROM projects WHERE id = ?")
        .bind(&input.project_id)
        .fetch_optional(state.0.as_ref())
        .await?;
    if project_exists.is_none() {
        return Err(ApiError::bad_request("Choose a valid project."));
    }
    let modified_at = now();
    let result = sqlx::query("UPDATE tasks SET title = ?, project_id = ?, description = ?, scratchpad = ?, modified_at = ? WHERE id = ? AND completed_at IS NULL")
        .bind(&title).bind(&input.project_id).bind(&input.description).bind(&input.scratchpad).bind(&modified_at).bind(&id).execute(state.0.as_ref()).await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    let task = sqlx::query_as::<_, Task>("SELECT id, title, description, scratchpad, project_id, position, created_at, modified_at, completed_at FROM tasks WHERE id = ?")
        .bind(id).fetch_one(state.0.as_ref()).await?;
    Ok(Json(task))
}

async fn complete_task(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Task>, ApiError> {
    let completed_at = now();
    let result = sqlx::query(
        "UPDATE tasks SET completed_at = ?, modified_at = ? WHERE id = ? AND completed_at IS NULL",
    )
    .bind(&completed_at)
    .bind(&completed_at)
    .bind(&id)
    .execute(state.0.as_ref())
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    let task = sqlx::query_as::<_, Task>("SELECT id, title, description, scratchpad, project_id, position, created_at, modified_at, completed_at FROM tasks WHERE id = ?")
        .bind(id).fetch_one(state.0.as_ref()).await?;
    Ok(Json(task))
}

async fn restore_task(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Task>, ApiError> {
    let mut tx = state.0.begin().await?;
    sqlx::query("UPDATE tasks SET position = position + 1 WHERE completed_at IS NULL")
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query(
        "UPDATE tasks SET completed_at = NULL, position = 0, modified_at = ? WHERE id = ? AND completed_at IS NOT NULL",
    )
    .bind(now())
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    tx.commit().await?;
    let task = sqlx::query_as::<_, Task>("SELECT id, title, description, scratchpad, project_id, position, created_at, modified_at, completed_at FROM tasks WHERE id = ?")
        .bind(id).fetch_one(state.0.as_ref()).await?;
    Ok(Json(task))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReorderTask {
    target_task_id: Option<String>,
    after: bool,
}

async fn reorder_task(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<ReorderTask>,
) -> Result<Json<Vec<String>>, ApiError> {
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM tasks WHERE completed_at IS NULL ORDER BY position",
    )
    .fetch_all(state.0.as_ref())
    .await?;
    let reordered = relocate(&ids, &id, input.target_task_id.as_deref(), input.after)
        .ok_or_else(ApiError::not_found)?;
    let mut tx = state.0.begin().await?;
    for (position, task_id) in reordered.iter().enumerate() {
        sqlx::query("UPDATE tasks SET position = ? WHERE id = ?")
            .bind(position as i64)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(reordered))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<Task>>, ApiError> {
    let pattern = format!("%{}%", query.q.trim());
    let tasks = sqlx::query_as::<_, Task>("SELECT id, title, description, scratchpad, project_id, position, created_at, modified_at, completed_at FROM tasks WHERE title LIKE ? OR description LIKE ? OR scratchpad LIKE ? ORDER BY completed_at IS NOT NULL, position, completed_at DESC")
        .bind(&pattern).bind(&pattern).bind(&pattern).fetch_all(state.0.as_ref()).await?;
    Ok(Json(tasks))
}

#[derive(Deserialize)]
struct SavePreference {
    value: String,
}

async fn save_preference(
    Path(key): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<SavePreference>,
) -> Result<StatusCode, ApiError> {
    sqlx::query("INSERT INTO preferences (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key).bind(input.value).execute(state.0.as_ref()).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn required(value: &str, name: &str) -> Result<String, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(ApiError::bad_request(&format!("{name} is required.")))
    } else {
        Ok(trimmed.to_owned())
    }
}

struct ApiError {
    status: StatusCode,
    message: String,
}
impl ApiError {
    fn bad_request(message: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "The task was not found.".into(),
        }
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        eprintln!("database error: {error}");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "The local database could not complete that action.".into(),
        }
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(serde_json::json!({ "message": self.message })),
        )
            .into_response()
    }
}
