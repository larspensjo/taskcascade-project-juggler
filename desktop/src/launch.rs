use axum::Router;
use std::{
    env,
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use taskcascade_backend::app::AppState;

use crate::{
    assets::{self, AssetSource, FrontendDist},
    bridge::{launch_url, serve_request, SCHEME},
};

pub fn compose<S: AssetSource>(state: AppState, assets: S) -> Router {
    taskcascade_backend::app::router(state).fallback_service(assets::router(assets))
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let data_dir = data_directory(env::var_os("TASKCASCADE_DATA_DIR"))?;
    #[cfg(debug_assertions)]
    if FrontendDist::get("index.html").is_none() {
        return Err("frontend not built — run npm run build in frontend/".into());
    }

    let router = Arc::new(OnceLock::<Router>::new());
    let protocol_router = Arc::clone(&router);
    let protocol_runtime = runtime.handle().clone();
    let setup_runtime = runtime.handle().clone();
    let state = Arc::new(OnceLock::<AppState>::new());
    let setup_state = Arc::clone(&state);
    let app = tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol(SCHEME, move |_context, request, responder| {
            let router = protocol_router
                .get()
                .expect("setup initializes router before creating window")
                .clone();
            protocol_runtime.spawn(async move {
                responder.respond(serve_request(router, request).await);
            });
        })
        .setup(move |app| {
            let connected = setup_runtime.block_on(async {
                tokio::fs::create_dir_all(&data_dir).await?;
                let url = format!("sqlite://{}", data_dir.join("taskcascade.sqlite").display());
                Ok::<_, Box<dyn std::error::Error>>(AppState::connect(&url).await?)
            })?;
            let app_router = compose(connected.clone(), FrontendDist);
            setup_state
                .set(connected)
                .unwrap_or_else(|_| unreachable!("setup runs once"));
            router
                .set(app_router)
                .unwrap_or_else(|_| unreachable!("setup runs once"));
            tauri::WebviewWindowBuilder::new(app, "main", launch_url())
                .title("TaskCascade (dev)")
                .inner_size(1280.0, 800.0)
                .min_inner_size(900.0, 560.0)
                // Mirrors frontend/src/styles.css --canvas.
                .background_color(tauri::utils::config::Color(0x0a, 0x0b, 0x0d, 0xff))
                .disable_drag_drop_handler()
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run_return(|_handle, _event| {});
    drop(state);
    Ok(())
}

fn data_directory(
    override_dir: Option<std::ffi::OsString>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    match override_dir {
        Some(value) => {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err("TASKCASCADE_DATA_DIR must be an absolute path".into());
            }
            Ok(path)
        }
        None => Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.local/dev-data")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_directory_is_anchored_and_override_must_be_absolute() {
        let default = data_directory(None).unwrap();
        assert!(default.is_absolute());
        assert_eq!(
            default,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.local/dev-data")
        );
        assert!(data_directory(Some("relative".into())).is_err());
        let scratch = tempfile::tempdir().unwrap();
        assert_eq!(
            data_directory(Some(scratch.path().into())).unwrap(),
            scratch.path()
        );
    }
}
