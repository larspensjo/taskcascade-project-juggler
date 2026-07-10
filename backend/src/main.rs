use std::{env, path::PathBuf};

use taskcascade_backend::app;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = env::var("TASKCASCADE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs_path("USERPROFILE")
                .map(|home| home.join("Documents").join("TaskCascade"))
                .unwrap_or_else(|| PathBuf::from(".local"))
        });
    tokio::fs::create_dir_all(&data_dir).await?;
    let database_url = format!("sqlite://{}", data_dir.join("taskcascade.sqlite").display());
    let state = app::AppState::connect(&database_url).await?;
    let port = env::var("TASKCASCADE_PORT").unwrap_or_else(|_| "8080".into());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    println!("TaskCascade listening at http://127.0.0.1:{port}");
    axum::serve(listener, app::router(state)).await?;
    Ok(())
}

fn dirs_path(variable: &str) -> Option<PathBuf> {
    env::var_os(variable).map(PathBuf::from)
}
