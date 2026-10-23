use axum::Router;
use std::{
    env,
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};
use taskcascade_backend::app::{AppState, OpenError, OpenMode};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

use crate::{
    assets::{self, AssetSource, FrontendDist},
    bridge::{launch_url, serve_tracked_request, InFlight, SCHEME},
    close_flush::{
        self, CloseCoordinator, CloseEvent, CloseStep, EvalProbes, Outcome, ACK_DEADLINE,
        BEFORE_CLOSE_SCRIPT, FLUSH_BACKSTOP,
    },
    content_security_policy,
    data_location::{self, LocationInputs},
    diagnostics::Diagnostics,
    identity::{self, Identity},
    lifecycle::{Lifecycle, SHUTDOWN_DEADLINE},
    navigation::{self, NavigationDecision},
    startup_failure::{self, PanicReport},
};
use tauri_plugin_window_state::StateFlags;

pub fn compose<S: AssetSource>(
    state: AppState,
    assets: S,
    coordinator: Arc<CloseCoordinator>,
    events: tokio::sync::mpsc::Sender<CloseEvent>,
) -> Router {
    taskcascade_backend::app::router(state)
        .merge(close_flush::router(coordinator, events))
        .fallback_service(assets::router(assets))
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let diagnostics = Arc::new(Diagnostics::default());
    let lifecycle = Arc::new(Lifecycle::new(Arc::clone(&diagnostics)));
    let panic_report = Arc::new(PanicReport::default());
    let previous_hook = panic::take_hook();
    panic_report.install_hook(Arc::clone(&diagnostics));
    let mut runtime = None;
    let mut database = "unresolved (data location failed)".to_owned();
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        run_inner(
            &mut runtime,
            &mut database,
            &diagnostics,
            &lifecycle,
            &panic_report,
        )
    }));
    let result = result.unwrap_or_else(|_| Err(panic_report.message().into()));
    let shutdown = panic::catch_unwind(AssertUnwindSafe(|| {
        if let Some(runtime) = runtime.take() {
            runtime.block_on(lifecycle.shutdown(SHUTDOWN_DEADLINE));
            runtime.shutdown_timeout(lifecycle.remaining());
        }
    }));
    panic::set_hook(previous_hook);
    let result = finish_run(result, shutdown.err().map(|_| panic_report.message()));
    if let Err(error) = &result {
        let message = startup_failure::failure_message(
            panic_report.window_loop_started(),
            &error.to_string(),
            &database,
            &diagnostics.path_description(),
        );
        diagnostics.log(&message);
        startup_failure::show("TaskCascade failure", &message);
    }
    result
}

fn finish_run(
    result: Result<(), Box<dyn std::error::Error>>,
    shutdown_panic: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    match (result, shutdown_panic) {
        (Err(error), Some(panic)) => {
            Err(format!("{error}\nShutdown also panicked: {panic}").into())
        }
        (Ok(()), Some(panic)) => Err(panic.into()),
        (result, None) => result,
    }
}

fn database_open_message(error: OpenError) -> String {
    match error {
        OpenError::Missing(path) => format!(
            "No task database at {}.\nTaskCascade does not create an empty database on its own. If your tasks were\nmoved, put taskcascade.sqlite back. To start with an empty database, start\nTaskCascade once with TASKCASCADE_CREATE_DATABASE=1 (scripts\\install.ps1 -InitDatabase).",
            path.display(),
        ),
        error => error.to_string(),
    }
}

fn run_inner(
    runtime: &mut Option<tokio::runtime::Runtime>,
    database: &mut String,
    diagnostics: &Arc<Diagnostics>,
    lifecycle: &Arc<Lifecycle>,
    panic_report: &Arc<PanicReport>,
) -> Result<(), Box<dyn std::error::Error>> {
    let inputs = LocationInputs {
        data_dir_override: env::var_os("TASKCASCADE_DATA_DIR"),
        create_opt_in: env::var_os("TASKCASCADE_CREATE_DATABASE").is_some_and(|value| value == "1"),
        local_app_data: env::var_os("LOCALAPPDATA").map(PathBuf::from),
        debug_build: cfg!(debug_assertions),
        dev_default: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.local/dev-data"),
    };
    let requested = data_location::requested_directory(&inputs)
        .ok()
        .or_else(|| inputs.data_dir_override.as_ref().map(PathBuf::from));
    if let Some(requested) = &requested {
        *database = requested.join("taskcascade.sqlite").display().to_string();
    }
    let describe_location_failure = |error: data_location::LocationError| {
        format!(
            "{error}\nRequested directory: {}\nReal directory: {}",
            requested.as_ref().map_or_else(
                || "unresolved".to_owned(),
                |path| path.display().to_string()
            ),
            inputs.local_app_data.as_deref().map_or_else(
                || "unresolved (LOCALAPPDATA is not set)".to_owned(),
                |path| data_location::real_directory(path).display().to_string()
            ),
        )
    };
    let location = data_location::resolve(&inputs).map_err(&describe_location_failure)?;
    *database = location.database.display().to_string();
    let identity = identity::identity_for(&location);
    let canonical_database = data_location::filesystem_identity(&location.database)
        .map_err(describe_location_failure)?
        .canonical;
    diagnostics.initialize(&location, &canonical_database, inputs.debug_build);
    log_identity(diagnostics, identity);
    *runtime = Some(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?,
    );
    let runtime = runtime
        .as_ref()
        .ok_or("desktop runtime was not initialized")?;
    #[cfg(debug_assertions)]
    if FrontendDist::get("index.html").is_none() {
        return Err("frontend not built — run npm run build in frontend/".into());
    }

    let router = Arc::new(OnceLock::<Router>::new());
    let coordinator = Arc::new(CloseCoordinator::default());
    let (events, close_events) = tokio::sync::mpsc::channel(64);
    let setup_coordinator = Arc::clone(&coordinator);
    let setup_events = events.clone();
    let eval_probes = Arc::new(EvalProbes::default());
    let page_eval_probes = Arc::clone(&eval_probes);
    let protocol_router = Arc::clone(&router);
    let protocol_in_flight = lifecycle.in_flight();
    let protocol_runtime = runtime.handle().clone();
    let setup_runtime = runtime.handle().clone();
    let setup_lifecycle = Arc::clone(lifecycle);
    let setup_outcome = Arc::new(OnceLock::new());
    let captured_outcome = Arc::clone(&setup_outcome);
    let setup_panic_report = Arc::clone(panic_report);
    let setup_diagnostics = Arc::clone(diagnostics);
    let single_instance_diagnostics = Arc::clone(diagnostics);
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            move |app, _argv, _cwd| {
                let Some(window) = app.get_webview_window("main") else {
                    single_instance_diagnostics
                        .log("second launch arrived before main window existed");
                    return;
                };
                let mut focused = true;
                for (operation, result) in [
                    ("unminimize", window.unminimize()),
                    ("show", window.show()),
                    ("focus", window.set_focus()),
                ] {
                    if let Err(error) = result {
                        focused = false;
                        single_instance_diagnostics.log(&format!(
                            "second launch could not {operation} main window: {error}"
                        ));
                    }
                }
                if focused {
                    single_instance_diagnostics
                        .log("second launch handed off; focused main window");
                }
            },
        ))
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::all() - StateFlags::VISIBLE)
                .build(),
        )
        .register_asynchronous_uri_scheme_protocol(SCHEME, move |_context, request, responder| {
            let router = Arc::clone(&protocol_router);
            let in_flight = Arc::clone(&protocol_in_flight);
            protocol_runtime.spawn(async move {
                responder.respond(serve_protocol_request(&router, request, &in_flight).await);
            });
        })
        .setup(move |app| {
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                let connected = setup_runtime.block_on(async {
                    if location.may_create {
                        tokio::fs::create_dir_all(&location.directory).await?;
                    }
                    let mode = if location.may_create {
                        OpenMode::CreateIfMissing
                    } else {
                        OpenMode::ExistingOnly
                    };
                    let state = AppState::open(&location.database, mode)
                        .await
                        .map_err(database_open_message)?;
                    setup_diagnostics
                        .log(&format!("database opened {}", location.database.display()));
                    setup_lifecycle.set_state(state.clone()).await;
                    Ok::<_, Box<dyn std::error::Error>>(state)
                })?;
                router
                    .set(compose(connected, FrontendDist, Arc::clone(&setup_coordinator), setup_events.clone()))
                    .map_err(|_| "desktop router was already initialized")?;
                let navigation_app = app.handle().clone();
                let navigation_runtime = setup_runtime.clone();
                let navigation_diagnostics = Arc::clone(&setup_diagnostics);
                let new_window_app = app.handle().clone();
                let new_window_runtime = setup_runtime.clone();
                let new_window_diagnostics = Arc::clone(&setup_diagnostics);
                let probe_runtime = setup_runtime.clone();
                let probe_diagnostics = Arc::clone(&setup_diagnostics);
                let window = tauri::WebviewWindowBuilder::new(app, "main", launch_url())
                    .title(identity.title)
                    .inner_size(1280.0, 800.0)
                    .min_inner_size(900.0, 560.0)
                    .visible(false)
                    // Mirrors frontend/src/styles.css --canvas.
                    .background_color(tauri::utils::config::Color(0x0a, 0x0b, 0x0d, 0xff))
                    .disable_drag_drop_handler()
                    .devtools(cfg!(debug_assertions))
                    .on_page_load(move |window, payload| {
                        if payload.event() != tauri::webview::PageLoadEvent::Finished { return; }
                        let generation = page_eval_probes.begin();
                        if let Err(error) = window.eval(format!("fetch('/desktop/eval-ok?generation={generation}', {{ method: 'POST' }})")) {
                            probe_diagnostics.log(&format!("WARNING: eval self-test could not run: {error}"));
                        }
                        let eval_probes = Arc::clone(&page_eval_probes);
                        let diagnostics = Arc::clone(&probe_diagnostics);
                        probe_runtime.spawn(async move {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            if !eval_probes.finish(generation) {
                                diagnostics.log("WARNING: eval self-test did not answer within 5 s; close handshake may be unavailable");
                            }
                        });
                    })
                    .on_navigation(move |url| {
                        allow_navigation(
                            &navigation_runtime,
                            &navigation_app,
                            &navigation_diagnostics,
                            url,
                        )
                    })
                    .on_new_window(move |url, _features| {
                        allow_navigation(
                            &new_window_runtime,
                            &new_window_app,
                            &new_window_diagnostics,
                            &url,
                        );
                        tauri::webview::NewWindowResponse::Deny
                    })
                    .build()?;
                #[cfg(all(windows, not(debug_assertions)))]
                if let Err(error) =
                    crate::webview_settings::apply(&window, Arc::clone(&setup_diagnostics))
                {
                    setup_diagnostics.log(&format!("failed to apply WebView2 settings: {error}"));
                }
                let show_diagnostics = Arc::clone(&setup_diagnostics);
                // Setup runs this synchronously on the main thread; build already restored
                // window state through attach_window's on_window_ready hook.
                app.run_on_main_thread(move || {
                    if let Err(error) = window.show() {
                        show_diagnostics
                            .log(&format!("failed to show restored desktop window: {error}"));
                    }
                })?;
                Ok(())
            })();
            // Tauri runs setup in run_return and panics if this hook returns Err.
            // Capture ordinary failures so run can report them after the loop exits.
            let outcome = captured_outcome.get_or_init(|| {
                SetupOutcome::from_result(result.map_err(|error| error.to_string()))
            });
            if outcome.window_loop_started() {
                setup_panic_report.mark_window_loop_started();
            } else {
                app.handle().exit(1);
            }
            Ok(())
        });
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = identity.identifier.into();
    let app = app.build(context)?;
    runtime.spawn(drive_close(
        app.handle().clone(),
        Arc::clone(&coordinator),
        events.clone(),
        close_events,
        lifecycle.in_flight(),
        Arc::clone(diagnostics),
        eval_probes,
    ));
    let exit_lifecycle = Arc::clone(lifecycle);
    let exit_runtime = runtime.handle().clone();
    app.run_return(move |handle, event| match event {
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if label == "main" => match coordinator.close_requested() {
            CloseStep::StartHandshake => {
                api.prevent_close();
                let attempt = coordinator.attempt();
                let evaluated = handle
                    .get_webview_window("main")
                    .is_some_and(|window| window.eval(BEFORE_CLOSE_SCRIPT).is_ok());
                if !evaluated {
                    let events = events.clone();
                    exit_runtime.spawn(async move {
                        let _ = events.send(CloseEvent::Outcome(Outcome::Error)).await;
                    });
                }
                arm_close_timer(
                    &exit_runtime,
                    events.clone(),
                    ACK_DEADLINE,
                    CloseEvent::AckDeadline(attempt),
                );
            }
            CloseStep::Prevent => api.prevent_close(),
            _ => {}
        },
        tauri::RunEvent::Exit => {
            exit_runtime.block_on(exit_lifecycle.shutdown(SHUTDOWN_DEADLINE));
        }
        _ => {}
    });
    runtime.block_on(lifecycle.shutdown(SHUTDOWN_DEADLINE));
    if let Some(SetupOutcome::Failed(error)) = setup_outcome.get() {
        return Err(error.clone().into());
    }
    Ok(())
}

fn arm_close_timer(
    runtime: &tokio::runtime::Handle,
    events: tokio::sync::mpsc::Sender<CloseEvent>,
    delay: Duration,
    event: CloseEvent,
) {
    runtime.spawn(async move {
        tokio::time::sleep(delay).await;
        let _ = events.send(event).await;
    });
}

async fn drive_close(
    app: tauri::AppHandle,
    coordinator: Arc<CloseCoordinator>,
    events: tokio::sync::mpsc::Sender<CloseEvent>,
    mut receiver: tokio::sync::mpsc::Receiver<CloseEvent>,
    in_flight: Arc<InFlight>,
    diagnostics: Arc<Diagnostics>,
    eval_probes: Arc<EvalProbes>,
) {
    while let Some(event) = receiver.recv().await {
        let step = match event {
            CloseEvent::Ack(attempt) => coordinator.ack(attempt),
            CloseEvent::Outcome(outcome) => {
                let step = coordinator.outcome(outcome);
                if outcome == Outcome::Failed && step == CloseStep::StayOpen {
                    diagnostics.log("close: page reported failure; window stays open");
                }
                step
            }
            CloseEvent::AckDeadline(attempt) => coordinator.ack_deadline(attempt),
            CloseEvent::Backstop(attempt) => coordinator.backstop(attempt),
            CloseEvent::Answer(attempt, answer) => coordinator.answer(attempt, answer),
            CloseEvent::EvalOk(generation) => {
                eval_probes.answer(generation);
                diagnostics.log("eval self-test ok");
                continue;
            }
        };
        match step {
            CloseStep::ArmBackstop => arm_close_timer(
                &tokio::runtime::Handle::current(),
                events.clone(),
                FLUSH_BACKSTOP,
                CloseEvent::Backstop(coordinator.attempt()),
            ),
            CloseStep::AskCloseAnyway => {
                let answer_events = events.clone();
                let runtime = tokio::runtime::Handle::current();
                let attempt = coordinator.attempt();
                let question_app = app.clone();
                if let Err(error) = app.run_on_main_thread(move || {
                    let answer = question_app
                        .get_webview_window("main")
                        .is_some_and(|window| ask_close_anyway(&window));
                    runtime.spawn(async move {
                        let _ = answer_events
                            .send(CloseEvent::Answer(attempt, answer))
                            .await;
                    });
                }) {
                    diagnostics.log(&format!("could not show close question: {error}"));
                    let _ = coordinator.answer(attempt, false);
                }
            }
            CloseStep::BeginShutdown(reason) => {
                diagnostics.log(match reason {
                    "saved" => "close: page confirmed all edits saved",
                    "discarded" => "close: user discarded unsaved changes",
                    "no page answered" => "close: no page answered",
                    _ => "close: closed without confirmation",
                });
                in_flight.stop_admitting();
                app.exit(0);
            }
            _ => {}
        }
    }
}

#[cfg(windows)]
fn ask_close_anyway(window: &tauri::WebviewWindow) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO,
    };
    let title: Vec<u16> = "TaskCascade".encode_utf16().chain(Some(0)).collect();
    let message: Vec<u16> = "TaskCascade could not confirm that your latest edits were saved. Close anyway? Unsaved changes may be lost.".encode_utf16().chain(Some(0)).collect();
    let Ok(owner) = window.hwnd() else {
        return false;
    };
    // SAFETY: Called on the main thread with the live main window's HWND;
    // both UTF-16 buffers are terminated and live throughout the modal call.
    unsafe {
        MessageBoxW(
            owner.0,
            message.as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        ) == IDYES
    }
}

#[cfg(not(windows))]
fn ask_close_anyway(_window: &tauri::WebviewWindow) -> bool {
    false
}

fn log_identity(diagnostics: &Diagnostics, identity: Identity) {
    diagnostics.log(&format!(
        "identity identifier={} title={}",
        identity.identifier, identity.title
    ));
}

fn allow_navigation(
    runtime: &tokio::runtime::Handle,
    app: &tauri::AppHandle,
    diagnostics: &Arc<Diagnostics>,
    url: &url::Url,
) -> bool {
    match navigation::classify(url) {
        NavigationDecision::Allow => true,
        NavigationDecision::OpenExternally => {
            let app = app.clone();
            let diagnostics = Arc::clone(diagnostics);
            let url = url.clone();
            // tauri-runtime-wry 2.12.3 runs run_on_main_thread inline on the UI thread;
            // hop through a worker so the open is queued after the navigation callback.
            runtime.spawn(async move {
                let opener_app = app.clone();
                let opener_diagnostics = Arc::clone(&diagnostics);
                if let Err(error) = app.run_on_main_thread(move || {
                    if let Err(error) = opener_app.opener().open_url(url.as_str(), None::<&str>) {
                        opener_diagnostics.log(&format!(
                            "failed to open external navigation {}: {error}",
                            navigation::url_for_log(url.as_str()),
                        ));
                    }
                }) {
                    diagnostics.log(&format!("failed to schedule external navigation: {error}"));
                }
            });
            false
        }
        NavigationDecision::Block => {
            diagnostics.log(&format!(
                "blocked navigation {}",
                navigation::url_for_log(url.as_str()),
            ));
            false
        }
    }
}

enum SetupOutcome {
    Started,
    Failed(String),
}

impl SetupOutcome {
    fn from_result(result: Result<(), String>) -> Self {
        match result {
            Ok(()) => Self::Started,
            Err(error) => Self::Failed(error),
        }
    }

    fn window_loop_started(&self) -> bool {
        matches!(self, Self::Started)
    }
}

async fn serve_protocol_request(
    router: &OnceLock<Router>,
    request: http::Request<Vec<u8>>,
    in_flight: &InFlight,
) -> http::Response<Vec<u8>> {
    match router.get() {
        Some(router) => serve_tracked_request(router.clone(), request, in_flight).await,
        None => http::Response::builder()
            .status(http::StatusCode::SERVICE_UNAVAILABLE)
            .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .header(
                http::header::CONTENT_SECURITY_POLICY,
                content_security_policy::VALUE,
            )
            .body(b"TaskCascade is starting.".to_vec())
            .expect("fixed startup response"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_database_message_gives_desktop_creation_guidance() {
        let path = PathBuf::from("scratch/tasks.sqlite");
        assert_eq!(
            database_open_message(OpenError::Missing(path.clone())),
            format!(
                "No task database at {}.\nTaskCascade does not create an empty database on its own. If your tasks were\nmoved, put taskcascade.sqlite back. To start with an empty database, start\nTaskCascade once with TASKCASCADE_CREATE_DATABASE=1 (scripts\\install.ps1 -InitDatabase).",
                path.display(),
            )
        );
    }

    #[test]
    fn startup_error_survives_a_shutdown_panic() {
        let error = finish_run(
            Err("database open failed".into()),
            Some("runtime teardown failed".to_owned()),
        )
        .unwrap_err();
        let message = startup_failure::failure_message(
            false,
            &error.to_string(),
            "scratch/tasks.sqlite",
            "scratch/logs/desktop.log",
        );
        assert!(message.starts_with("TaskCascade could not start."));
        assert!(message.contains("database open failed"));
        assert!(message.contains("Shutdown also panicked: runtime teardown failed"));
    }

    #[test]
    fn setup_outcome_preserves_failure_and_marks_only_success_as_started() {
        let failure = SetupOutcome::from_result(Err("database open failed".to_owned()));
        assert!(!failure.window_loop_started());
        let SetupOutcome::Failed(error) = &failure else {
            panic!("setup failure must remain available after run_return");
        };
        let message = startup_failure::failure_message(
            failure.window_loop_started(),
            error,
            "scratch/tasks.sqlite",
            "scratch/logs/desktop.log",
        );
        assert!(message.starts_with("TaskCascade could not start."));
        assert!(message.contains("database open failed"));

        let success = SetupOutcome::from_result(Ok(()));
        assert!(success.window_loop_started());
        let message = startup_failure::failure_message(
            success.window_loop_started(),
            "later panic",
            "scratch/tasks.sqlite",
            "scratch/logs/desktop.log",
        );
        assert!(message.starts_with("TaskCascade stopped unexpectedly."));
    }

    #[tokio::test]
    async fn scheme_returns_service_unavailable_before_setup() {
        let response = serve_protocol_request(
            &OnceLock::new(),
            http::Request::new(Vec::new()),
            &InFlight::default(),
        )
        .await;
        assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers()[http::header::CONTENT_SECURITY_POLICY],
            content_security_policy::VALUE
        );
        assert_eq!(
            response.headers()[http::header::CONTENT_TYPE],
            "text/plain; charset=utf-8"
        );
    }
}
