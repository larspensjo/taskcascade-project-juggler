use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use taskcascade_backend::app::AppState;
use tokio::sync::Mutex;

use crate::{bridge::InFlight, diagnostics::Diagnostics};

pub const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(5);
const RUNTIME_RESERVE: Duration = Duration::from_millis(500);
const DRAIN_CAP: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShutdownReport {
    pub drained: bool,
    pub pool_closed: bool,
    pub elapsed: Duration,
    pub exhausted_at: Option<&'static str>,
}

#[derive(Default)]
struct ShutdownState {
    state: Option<AppState>,
    report: Option<ShutdownReport>,
}

#[derive(Default)]
pub struct Lifecycle {
    inner: Mutex<ShutdownState>,
    deadline: OnceLock<Instant>,
    diagnostics: Arc<Diagnostics>,
    in_flight: Arc<InFlight>,
}

impl Lifecycle {
    pub fn new(diagnostics: Arc<Diagnostics>) -> Self {
        Self {
            diagnostics,
            ..Self::default()
        }
    }

    pub async fn set_state(&self, state: AppState) {
        self.inner.lock().await.state = Some(state);
    }

    pub fn in_flight(&self) -> Arc<InFlight> {
        Arc::clone(&self.in_flight)
    }

    pub fn remaining(&self) -> Duration {
        self.deadline.get().map_or(SHUTDOWN_DEADLINE, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        })
    }

    /// Idempotent; concurrent or subsequent callers receive the first report.
    pub async fn shutdown(&self, deadline: Duration) -> ShutdownReport {
        let mut inner = self.inner.lock().await;
        if let Some(report) = &inner.report {
            return report.clone();
        }
        let started = Instant::now();
        self.deadline.get_or_init(|| started + deadline);
        let state = inner.state.take();
        let had_state = state.is_some();
        self.in_flight.stop_admitting();
        let drained = self
            .in_flight
            .wait_idle(DRAIN_CAP.min(self.remaining()))
            .await;
        let close_budget = self.remaining().saturating_sub(RUNTIME_RESERVE);
        let pool_closed = match state {
            Some(_) if close_budget.is_zero() => false,
            Some(state) => tokio::time::timeout(close_budget, state.close())
                .await
                .is_ok(),
            None => true,
        };
        let report = ShutdownReport {
            drained,
            pool_closed,
            elapsed: started.elapsed(),
            exhausted_at: if !drained {
                Some("drain")
            } else {
                (!pool_closed).then_some("pool close")
            },
        };
        self.diagnostics
            .log(&format!("shutdown report: {report:?}"));
        match report.exhausted_at {
            Some(stage) => self
                .diagnostics
                .log(&format!("shutdown: deadline exhausted at {stage}")),
            None if had_state => self.diagnostics.log("shutdown: database closed cleanly"),
            None => self.diagnostics.log("shutdown: no database was open"),
        }
        inner.report = Some(report.clone());
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bridge::serve_request, data_location::DataLocation};
    use http::{Request, StatusCode};
    use serde_json::{json, Value};
    use taskcascade_backend::app::{router, OpenMode};

    async fn fixture() -> (tempfile::TempDir, AppState, Lifecycle) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::open(&dir.path().join("tasks.sqlite"), OpenMode::CreateIfMissing)
            .await
            .unwrap();
        let diagnostics = Arc::new(Diagnostics::default());
        let location = DataLocation {
            directory: dir.path().to_owned(),
            database: dir.path().join("tasks.sqlite"),
            is_real: false,
            may_create: true,
        };
        diagnostics.initialize(&location, &location.database, true);
        let lifecycle = Lifecycle::new(diagnostics);
        lifecycle.set_state(state.clone()).await;
        (dir, state, lifecycle)
    }

    #[tokio::test]
    async fn held_request_exhausts_short_drain_budget_and_skips_pool_close() {
        let (_dir, state, lifecycle) = fixture().await;
        let in_flight = lifecycle.in_flight();
        let _held = in_flight.track().unwrap();
        let started = Instant::now();
        let report = lifecycle.shutdown(Duration::from_millis(500)).await;
        assert!(started.elapsed() < Duration::from_millis(700));
        assert!(!report.drained);
        assert!(!report.pool_closed);
        assert_eq!(report.exhausted_at, Some("drain"));
        assert!(in_flight.track().is_none());
        state.close().await;
    }

    #[tokio::test]
    async fn held_request_times_out_at_drain_cap_but_pool_closes_in_remaining_budget() {
        let (_dir, _state, lifecycle) = fixture().await;
        let in_flight = lifecycle.in_flight();
        let _held = in_flight.track().unwrap();
        let started = Instant::now();
        let report = lifecycle.shutdown(Duration::from_secs(3)).await;
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(report.elapsed >= Duration::from_secs(2));
        assert!(!report.drained);
        assert!(report.pool_closed);
        assert_eq!(report.exhausted_at, Some("drain"));
    }

    #[tokio::test]
    async fn normal_close_releases_file_and_preserves_committed_task() {
        let (dir, state, lifecycle) = fixture().await;
        let app = router(state);
        let response = serve_request(
            app.clone(),
            Request::builder()
                .uri("/api/bootstrap")
                .body(Vec::new())
                .unwrap(),
        )
        .await;
        let bootstrap: Value = serde_json::from_slice(response.body()).unwrap();
        let response = serve_request(app, Request::builder().method("POST").uri("/api/tasks").header("content-type", "application/json").body(json!({"title": "Committed before close", "projectId": bootstrap["projects"][0]["id"]}).to_string().into_bytes()).unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let report = lifecycle.shutdown(SHUTDOWN_DEADLINE).await;
        assert!(report.drained);
        assert!(report.pool_closed);
        assert_eq!(report.exhausted_at, None);
        let log = std::fs::read_to_string(dir.path().join("logs/taskcascade-desktop.log")).unwrap();
        assert!(log.contains("shutdown: database closed cleanly"));
        let path = dir.path().join("closed.sqlite");
        std::fs::rename(dir.path().join("tasks.sqlite"), &path).unwrap();
        let reopened = AppState::open(&path, OpenMode::ExistingOnly).await.unwrap();
        let response = serve_request(
            router(reopened.clone()),
            Request::builder()
                .uri("/api/bootstrap")
                .body(Vec::new())
                .unwrap(),
        )
        .await;
        let bootstrap: Value = serde_json::from_slice(response.body()).unwrap();
        assert_eq!(
            bootstrap["activeTasks"][0]["title"],
            "Committed before close"
        );
        reopened.close().await;
    }

    #[tokio::test]
    async fn runtime_reserve_exhausts_budget_before_pool_close_starts() {
        let (_dir, state, lifecycle) = fixture().await;
        let held = state.acquire_for_tests().await;
        let start = Instant::now();
        let report = lifecycle.shutdown(Duration::from_millis(500)).await;
        assert!(start.elapsed() < Duration::from_millis(700));
        assert!(!report.pool_closed);
        assert_eq!(report.exhausted_at, Some("pool close"));
        // The 500 ms runtime reserve leaves no budget to call close().
        // A new checkout proves that the pool was never asked to close.
        drop(state.acquire_for_tests().await);
        drop(held);
        state.close().await;
    }

    #[tokio::test]
    async fn held_connection_times_out_during_pool_close() {
        let (dir, state, lifecycle) = fixture().await;
        let held = state.acquire_for_tests().await;
        let start = Instant::now();
        let report = lifecycle.shutdown(Duration::from_millis(600)).await;
        assert!(start.elapsed() < Duration::from_millis(700));
        // The remaining ~100 ms is spent attempting close, rather than skipping it.
        assert!(report.elapsed >= Duration::from_millis(50));
        assert!(!report.pool_closed);
        assert_eq!(report.exhausted_at, Some("pool close"));
        let log = std::fs::read_to_string(dir.path().join("logs/taskcascade-desktop.log")).unwrap();
        assert!(log.contains("shutdown: deadline exhausted at pool close"));
        drop(held);
        state.close().await;
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let (_dir, _state, lifecycle) = fixture().await;
        let first = lifecycle.shutdown(SHUTDOWN_DEADLINE).await;
        let remaining = lifecycle.remaining();
        let start = std::time::Instant::now();
        let second = lifecycle.shutdown(Duration::from_secs(60)).await;
        assert!(start.elapsed() < Duration::from_millis(100));
        assert_eq!(first, second);
        assert!(lifecycle.remaining() <= remaining);
    }

    #[tokio::test]
    async fn setup_failure_without_state_shuts_down_once() {
        let lifecycle = Lifecycle::default();
        let first = lifecycle.shutdown(SHUTDOWN_DEADLINE).await;
        assert!(first.pool_closed);
        assert_eq!(first.exhausted_at, None);
        assert_eq!(lifecycle.shutdown(SHUTDOWN_DEADLINE).await, first);
        assert_eq!(lifecycle.shutdown(SHUTDOWN_DEADLINE).await, first);
    }
}
