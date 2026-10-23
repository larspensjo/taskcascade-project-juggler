use axum::{extract::Query, routing::post, Router};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc::Sender;

pub const CLOSE_ACK_PATH: &str = "/desktop/close-ack";
pub const CLOSE_READY_PATH: &str = "/desktop/close-ready";
pub const EVAL_PROBE_PATH: &str = "/desktop/eval-ok";
pub const BEFORE_CLOSE_SCRIPT: &str = "(async () => { \
  await fetch('/desktop/close-ack', { method: 'POST' }); \
  if (typeof window.taskcascadeBeforeClose !== 'function') { await fetch('/desktop/close-ready?outcome=no-page', { method: 'POST' }); return; } \
  try { await window.taskcascadeBeforeClose(); } catch (e) { await fetch('/desktop/close-ready?outcome=error', { method: 'POST' }); } })();";
pub const ACK_DEADLINE: Duration = Duration::from_secs(3);
pub const FLUSH_BACKSTOP: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Saved,
    Failed,
    Discarded,
    Error,
    NoPage,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CloseState {
    #[default]
    Open,
    AwaitingAck,
    Flushing,
    Asking,
    Closing,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseStep {
    StartHandshake,
    Prevent,
    ArmBackstop,
    StayOpen,
    AskCloseAnyway,
    BeginShutdown(&'static str),
    Ignore,
}
#[derive(Default)]
struct Inner {
    state: CloseState,
    attempt: u64,
}
#[derive(Default)]
pub struct CloseCoordinator {
    inner: Mutex<Inner>,
}
impl CloseCoordinator {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    pub fn attempt(&self) -> u64 {
        self.inner().attempt
    }
    pub fn close_requested(&self) -> CloseStep {
        let mut inner = self.inner();
        match inner.state {
            CloseState::Open => {
                inner.attempt += 1;
                inner.state = CloseState::AwaitingAck;
                CloseStep::StartHandshake
            }
            CloseState::Closing => CloseStep::Ignore,
            _ => CloseStep::Prevent,
        }
    }
    pub fn ack(&self, attempt: u64) -> CloseStep {
        let mut inner = self.inner();
        if inner.attempt != attempt || inner.state != CloseState::AwaitingAck {
            return CloseStep::Ignore;
        }
        inner.state = CloseState::Flushing;
        CloseStep::ArmBackstop
    }
    pub fn outcome(&self, outcome: Outcome) -> CloseStep {
        let mut inner = self.inner();
        if inner.state == CloseState::Closing {
            return CloseStep::Ignore;
        }
        // Only the page's explicit saved/discarded choices close from Open.
        if inner.state == CloseState::Open
            && !matches!(outcome, Outcome::Saved | Outcome::Discarded)
        {
            return CloseStep::Ignore;
        }
        match outcome {
            Outcome::Saved | Outcome::Discarded | Outcome::NoPage => {
                inner.state = CloseState::Closing;
                CloseStep::BeginShutdown(match outcome {
                    Outcome::Saved => "saved",
                    Outcome::Discarded => "discarded",
                    _ => "no page answered",
                })
            }
            Outcome::Failed => {
                inner.state = CloseState::Open;
                CloseStep::StayOpen
            }
            Outcome::Error => {
                inner.state = CloseState::Asking;
                CloseStep::AskCloseAnyway
            }
        }
    }
    pub fn ack_deadline(&self, attempt: u64) -> CloseStep {
        let mut inner = self.inner();
        if inner.attempt != attempt || inner.state != CloseState::AwaitingAck {
            return CloseStep::Ignore;
        }
        inner.state = CloseState::Closing;
        CloseStep::BeginShutdown("no page answered")
    }
    pub fn backstop(&self, attempt: u64) -> CloseStep {
        let mut inner = self.inner();
        if inner.attempt != attempt || inner.state != CloseState::Flushing {
            return CloseStep::Ignore;
        }
        inner.state = CloseState::Asking;
        CloseStep::AskCloseAnyway
    }
    pub fn answer(&self, attempt: u64, close_anyway: bool) -> CloseStep {
        let mut inner = self.inner();
        if inner.attempt != attempt || inner.state != CloseState::Asking {
            return CloseStep::Ignore;
        }
        if close_anyway {
            inner.state = CloseState::Closing;
            CloseStep::BeginShutdown("closed without confirmation")
        } else {
            inner.state = CloseState::Open;
            CloseStep::StayOpen
        }
    }
}

#[derive(Debug)]
pub enum CloseEvent {
    Ack(u64),
    Outcome(Outcome),
    AckDeadline(u64),
    Backstop(u64),
    Answer(u64, bool),
    EvalOk(u64),
}

#[derive(Default)]
pub struct EvalProbes {
    inner: Mutex<(u64, HashMap<u64, bool>)>,
}

impl EvalProbes {
    pub fn begin(&self) -> u64 {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        inner.0 += 1;
        let generation = inner.0;
        inner.1.insert(generation, false);
        generation
    }

    pub fn answer(&self, generation: u64) {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(answered) = inner.1.get_mut(&generation) {
            *answered = true;
        }
    }

    pub fn finish(&self, generation: u64) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .1
            .remove(&generation)
            .unwrap_or(false)
    }
}

pub fn router(coordinator: Arc<CloseCoordinator>, events: Sender<CloseEvent>) -> Router {
    let ack_events = events.clone();
    let ready_events = events.clone();
    Router::new()
        .route(
            CLOSE_ACK_PATH,
            post(move || {
                let events = ack_events.clone();
                let attempt = coordinator.attempt();
                async move {
                    let _ = events.send(CloseEvent::Ack(attempt)).await;
                    http::StatusCode::NO_CONTENT
                }
            }),
        )
        .route(
            CLOSE_READY_PATH,
            post(
                move |Query(query): Query<std::collections::HashMap<String, String>>| {
                    let events = ready_events.clone();
                    async move {
                        let outcome = match query.get("outcome").map(String::as_str) {
                            Some("saved") => Outcome::Saved,
                            Some("failed") => Outcome::Failed,
                            Some("discarded") => Outcome::Discarded,
                            Some("error") => Outcome::Error,
                            Some("no-page") => Outcome::NoPage,
                            _ => return http::StatusCode::BAD_REQUEST,
                        };
                        let _ = events.send(CloseEvent::Outcome(outcome)).await;
                        http::StatusCode::NO_CONTENT
                    }
                },
            ),
        )
        .route(
            EVAL_PROBE_PATH,
            post(move |Query(query): Query<HashMap<String, String>>| {
                let events = events.clone();
                async move {
                    let Some(generation) =
                        query.get("generation").and_then(|value| value.parse().ok())
                    else {
                        return http::StatusCode::BAD_REQUEST;
                    };
                    let _ = events.send(CloseEvent::EvalOk(generation)).await;
                    http::StatusCode::NO_CONTENT
                }
            }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn desktop_routes_report_events_and_return_no_content() {
        let coordinator = Arc::new(CloseCoordinator::default());
        coordinator.close_requested();
        let (events, mut received) = tokio::sync::mpsc::channel(16);
        let router = router(Arc::clone(&coordinator), events);
        for path in [
            CLOSE_ACK_PATH,
            "/desktop/close-ready?outcome=failed",
            "/desktop/close-ready?outcome=saved",
            "/desktop/close-ready?outcome=discarded",
            "/desktop/close-ready?outcome=error",
            "/desktop/close-ready?outcome=no-page",
            "/desktop/eval-ok?generation=7",
        ] {
            let response = crate::bridge::serve_request(
                router.clone(),
                http::Request::builder()
                    .method("POST")
                    .uri(path)
                    .body(Vec::new())
                    .unwrap(),
            )
            .await;
            assert_eq!(response.status(), http::StatusCode::NO_CONTENT);
            assert!(response.body().is_empty());
        }
        assert!(
            matches!(received.recv().await.unwrap(), CloseEvent::Ack(attempt) if attempt == coordinator.attempt())
        );
        for expected in [
            Outcome::Failed,
            Outcome::Saved,
            Outcome::Discarded,
            Outcome::Error,
            Outcome::NoPage,
        ] {
            assert!(
                matches!(received.recv().await.unwrap(), CloseEvent::Outcome(outcome) if outcome == expected)
            );
        }
        assert!(matches!(
            received.recv().await.unwrap(),
            CloseEvent::EvalOk(7)
        ));
        let response = crate::bridge::serve_request(
            router,
            http::Request::builder()
                .method("POST")
                .uri("/desktop/close-ready?outcome=typo")
                .body(Vec::new())
                .unwrap(),
        )
        .await;
        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn repeated_close_only_prevents_and_failure_disarms_both_timers() {
        let c = CloseCoordinator::default();
        assert_eq!(c.close_requested(), CloseStep::StartHandshake);
        let attempt = c.attempt();
        assert_eq!(c.close_requested(), CloseStep::Prevent);
        assert_eq!(c.ack(attempt), CloseStep::ArmBackstop);
        assert_eq!(c.close_requested(), CloseStep::Prevent);
        assert_eq!(c.outcome(Outcome::Failed), CloseStep::StayOpen);
        assert_eq!(c.ack_deadline(attempt), CloseStep::Ignore);
        assert_eq!(c.backstop(attempt), CloseStep::Ignore);
    }

    #[test]
    fn absent_page_closes_only_on_ack_deadline() {
        let c = CloseCoordinator::default();
        c.close_requested();
        assert_eq!(
            c.ack_deadline(c.attempt()),
            CloseStep::BeginShutdown("no page answered")
        );
        assert_eq!(c.close_requested(), CloseStep::Ignore);
    }

    #[test]
    fn acknowledged_page_backstop_asks_and_stale_attempts_are_ignored() {
        let c = CloseCoordinator::default();
        c.close_requested();
        let old = c.attempt();
        c.ack(old);
        assert_eq!(c.ack_deadline(old), CloseStep::Ignore);
        assert_eq!(c.backstop(old), CloseStep::AskCloseAnyway);
        assert_eq!(c.close_requested(), CloseStep::Prevent);
        assert_eq!(c.answer(old, false), CloseStep::StayOpen);
        assert_eq!(c.close_requested(), CloseStep::StartHandshake);
        assert_ne!(c.attempt(), old);
        assert_eq!(c.ack(old), CloseStep::Ignore);
        assert_eq!(c.ack_deadline(old), CloseStep::Ignore);
        assert_eq!(c.backstop(old), CloseStep::Ignore);
        c.ack(c.attempt());
        assert_eq!(c.outcome(Outcome::Error), CloseStep::AskCloseAnyway);
        assert_eq!(c.answer(old, true), CloseStep::Ignore);
        assert_eq!(c.answer(old, false), CloseStep::Ignore);
        assert_eq!(
            c.answer(c.attempt(), true),
            CloseStep::BeginShutdown("closed without confirmation")
        );
    }

    #[test]
    fn eval_answers_belong_to_their_load_even_when_deadlines_overlap() {
        let probes = EvalProbes::default();
        let first = probes.begin();
        let second = probes.begin();
        probes.answer(second);
        assert!(!probes.finish(first));
        assert!(probes.finish(second));

        let third = probes.begin();
        probes.answer(first);
        assert!(!probes.finish(third));

        let fourth = probes.begin();
        probes.answer(fourth);
        let fifth = probes.begin();
        assert!(probes.finish(fourth));
        assert!(!probes.finish(fifth));
    }

    #[test]
    fn explicit_outcomes_close_including_the_pages_own_dialog() {
        for (outcome, reason) in [
            (Outcome::Saved, "saved"),
            (Outcome::Discarded, "discarded"),
            (Outcome::NoPage, "no page answered"),
        ] {
            let c = CloseCoordinator::default();
            c.close_requested();
            c.ack(c.attempt());
            assert_eq!(c.outcome(outcome), CloseStep::BeginShutdown(reason));
        }
        for (outcome, reason) in [(Outcome::Saved, "saved"), (Outcome::Discarded, "discarded")] {
            assert_eq!(
                CloseCoordinator::default().outcome(outcome),
                CloseStep::BeginShutdown(reason)
            );
        }
    }
}
