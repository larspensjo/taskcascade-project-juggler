use axum::body::{to_bytes, Body};
use axum::Router;
use http::{header, HeaderValue, StatusCode, Uri};
use http::{Request, Response};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::sync::Notify;
use tower::ServiceExt;

use crate::content_security_policy;

pub const SCHEME: &str = "taskcascade";
pub const PAGE_ORIGIN: &str = "http://taskcascade.localhost";

// Admission and the count share an atomic word so stopping admission cannot
// race with a request incrementing the counter after the drain sees zero.
const STOPPED: usize = 1 << (usize::BITS - 1);
#[derive(Default)]
pub struct InFlight {
    state: AtomicUsize,
    idle: Notify,
}
pub struct InFlightGuard<'a>(&'a InFlight);
impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        if self.0.state.fetch_sub(1, Ordering::AcqRel) & !STOPPED == 1 {
            self.0.idle.notify_waiters();
        }
    }
}
impl InFlight {
    pub fn track(&self) -> Option<InFlightGuard<'_>> {
        self.state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                (state & STOPPED == 0).then_some(state + 1)
            })
            .ok()
            .map(|_| InFlightGuard(self))
    }
    pub fn stop_admitting(&self) {
        self.state.fetch_or(STOPPED, Ordering::AcqRel);
    }
    pub async fn wait_idle(&self, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, async {
            loop {
                let notified = self.idle.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.state.load(Ordering::Acquire) & !STOPPED == 0 {
                    return;
                }
                notified.await;
            }
        })
        .await
        .is_ok()
    }
}

pub async fn serve_tracked_request(
    router: Router,
    request: Request<Vec<u8>>,
    in_flight: &InFlight,
) -> Response<Vec<u8>> {
    let path = request.uri().path();
    let _guard = if path == "/api" || path.starts_with("/api/") {
        match in_flight.track() {
            Some(guard) => Some(guard),
            None => {
                return Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(
                        header::CONTENT_SECURITY_POLICY,
                        content_security_policy::VALUE,
                    )
                    .body(br#"{"message":"TaskCascade is closing."}"#.to_vec())
                    .expect("fixed closing response")
            }
        }
    } else {
        None
    };
    serve_request(router, request).await
}

pub fn launch_url() -> tauri::WebviewUrl {
    tauri::WebviewUrl::CustomProtocol(
        format!("{SCHEME}://localhost/")
            .parse()
            .expect("fixed launch URL"),
    )
}

/// Preserve method, headers and body while reducing the URI to path-and-query.
pub async fn serve_request(router: Router, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let (mut parts, body) = request.into_parts();
    parts.uri = parts.uri.path_and_query().map_or_else(
        || Uri::from_static("/"),
        |path| path.as_str().parse().expect("valid path-and-query"),
    );
    let response = match router
        .oneshot(Request::from_parts(parts, Body::from(body)))
        .await
    {
        Ok(response) => response,
        Err(never) => match never {},
    };
    let (mut parts, body) = response.into_parts();
    let body = match to_bytes(body, usize::MAX).await {
        Ok(body) => body.to_vec(),
        Err(error) => {
            eprintln!("Desktop bridge could not collect response body: {error}");
            let response = Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(header::CONTENT_TYPE, "application/json")
                .body(br#"{"message":"The desktop bridge could not read the response."}"#.to_vec())
                .expect("fixed error response");
            let (error_parts, body) = response.into_parts();
            parts = error_parts;
            body
        }
    };
    parts.headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(content_security_policy::VALUE),
    );
    Response::from_parts(parts, body)
}

#[cfg(test)]
mod tests {
    use super::{serve_tracked_request, InFlight};
    use axum::{
        body::{to_bytes, Body},
        extract::Query,
        routing::get,
    };
    use http::{header, StatusCode};
    use serde_json::{json, Value};
    use std::{sync::Arc, time::Duration};

    #[tokio::test]
    async fn admitted_api_request_remains_counted_until_its_response_finishes() {
        let requests = Arc::new(InFlight::default());
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let handler_entered = Arc::clone(&entered);
        let handler_release = Arc::clone(&release);
        let app = axum::Router::new().route(
            "/api/held",
            get(move || async move {
                handler_entered.notify_one();
                handler_release.notified().await;
                "committed"
            }),
        );
        let tracked = Arc::clone(&requests);
        let running = tokio::spawn(async move {
            serve_tracked_request(
                app,
                http::Request::builder()
                    .uri("http://taskcascade.localhost/api/held")
                    .body(Vec::new())
                    .unwrap(),
                &tracked,
            )
            .await
        });
        entered.notified().await;
        requests.stop_admitting();
        assert!(!requests.wait_idle(Duration::from_millis(10)).await);
        release.notify_one();
        assert_eq!(running.await.unwrap().body(), b"committed");
        assert!(requests.wait_idle(Duration::from_secs(1)).await);
    }

    #[tokio::test]
    async fn in_flight_waits_for_last_guard_and_times_out_if_held() {
        let requests = Arc::new(InFlight::default());
        let first = requests.track().unwrap();
        let last = requests.track().unwrap();
        drop(first);
        assert!(!requests.wait_idle(Duration::from_millis(10)).await);
        let waiter = Arc::clone(&requests);
        let waiting = tokio::spawn(async move { waiter.wait_idle(Duration::from_secs(1)).await });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        drop(last);
        assert!(waiting.await.unwrap());
    }

    #[tokio::test]
    async fn stopped_admission_is_json_503_and_desktop_routes_are_never_counted() {
        let requests = Arc::new(InFlight::default());
        let checked = Arc::clone(&requests);
        let app = axum::Router::new().route(
            "/desktop/test",
            get(move || async move {
                assert!(checked.wait_idle(Duration::ZERO).await);
                http::StatusCode::NO_CONTENT
            }),
        );
        for stopped in [false, true] {
            if stopped {
                requests.stop_admitting();
            }
            let response = serve_tracked_request(
                app.clone(),
                http::Request::builder()
                    .uri("/desktop/test")
                    .body(Vec::new())
                    .unwrap(),
                &requests,
            )
            .await;
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }
        assert!(requests.track().is_none());
        let response = serve_tracked_request(
            app,
            http::Request::builder()
                .uri("taskcascade://localhost/api/health")
                .body(Vec::new())
                .unwrap(),
            &requests,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers()[http::header::CONTENT_TYPE],
            "application/json"
        );
        assert!(serde_json::from_slice::<Value>(response.body()).unwrap()["message"].is_string());
    }
    use taskcascade_backend::app::{self, AppState};
    use tower::ServiceExt;

    use super::*;
    use crate::{assets::tests::Fixture, content_security_policy, launch::compose};

    fn request(method: &str, path: &str, body: Value) -> Request<Vec<u8>> {
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_string().into_bytes())
            .unwrap()
    }

    async fn bare(router: Router, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
        let (parts, body) = request.into_parts();
        let response = router
            .oneshot(Request::from_parts(parts, Body::from(body)))
            .await
            .unwrap();
        let (parts, body) = response.into_parts();
        Response::from_parts(parts, to_bytes(body, usize::MAX).await.unwrap().to_vec())
    }

    #[tokio::test]
    async fn matches_bare_api_router_status_body_and_content_type() {
        let state = AppState::connect("sqlite::memory:").await.unwrap();
        let direct = app::router(state.clone());
        let bridged = compose(
            state,
            Fixture,
            Arc::default(),
            tokio::sync::mpsc::channel(16).0,
        );
        let bootstrap = bare(
            direct.clone(),
            request("GET", "/api/bootstrap", Value::Null),
        )
        .await;
        let project =
            serde_json::from_slice::<Value>(bootstrap.body()).unwrap()["projects"][0]["id"].clone();
        for (method, path, body, expected_status) in [
            ("GET", "/api/bootstrap", Value::Null, StatusCode::OK),
            (
                "POST",
                "/api/tasks",
                json!({"title": "Scratch task", "projectId": project}),
                StatusCode::OK,
            ),
            (
                "POST",
                "/api/tasks",
                json!({"title": "", "projectId": project}),
                StatusCode::BAD_REQUEST,
            ),
            (
                "PUT",
                "/api/preferences/projectFilter",
                json!({"value": "all"}),
                StatusCode::NO_CONTENT,
            ),
            ("GET", "/api/nope", Value::Null, StatusCode::NOT_FOUND),
        ] {
            let req = request(method, path, body);
            let expected = bare(direct.clone(), req.clone()).await;
            let actual = serve_request(bridged.clone(), req).await;
            assert_eq!(expected.status(), expected_status, "{method} {path}");
            assert_eq!(actual.status(), expected.status());
            assert_eq!(
                actual.headers().get(header::CONTENT_TYPE),
                expected.headers().get(header::CONTENT_TYPE)
            );
            if method == "POST" && expected_status == StatusCode::OK {
                // Separate creations have fresh UUIDs/timestamps. Compare all other fields.
                let mut expected: Value = serde_json::from_slice(expected.body()).unwrap();
                let mut actual: Value = serde_json::from_slice(actual.body()).unwrap();
                for key in ["id", "createdAt", "modifiedAt"] {
                    assert!(!actual[key].as_str().unwrap().is_empty());
                    assert!(!expected[key].as_str().unwrap().is_empty());
                    expected.as_object_mut().unwrap().remove(key);
                    actual.as_object_mut().unwrap().remove(key);
                }
                assert_eq!(actual, expected);
            } else {
                assert_eq!(actual.body(), expected.body());
            }
            if expected_status == StatusCode::BAD_REQUEST {
                assert_eq!(
                    serde_json::from_slice::<Value>(actual.body()).unwrap()["message"],
                    "Title is required."
                );
            }
            if expected_status == StatusCode::NO_CONTENT {
                assert!(actual.body().is_empty());
            }
        }
    }

    #[tokio::test]
    async fn uri_forms_preserve_path_and_query() {
        let app = Router::new().route(
            "/api/health",
            get(
                |uri: axum::http::Uri,
                 Query(query): Query<std::collections::BTreeMap<String, String>>| async move {
                    assert_eq!(uri, "/api/health?x=1");
                    query["x"].clone()
                },
            ),
        );
        for uri in [
            "taskcascade://localhost/api/health?x=1",
            "http://taskcascade.localhost/api/health?x=1",
            "/api/health?x=1",
        ] {
            let response = serve_request(app.clone(), request("GET", uri, Value::Null)).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.body(), b"1");
        }
    }

    #[tokio::test]
    async fn csp_is_on_assets_api_and_errors() {
        let state = AppState::connect("sqlite::memory:").await.unwrap();
        let app = compose(
            state,
            Fixture,
            Arc::default(),
            tokio::sync::mpsc::channel(16).0,
        );
        for path in ["/", "/assets/app.js", "/api/health", "/missing"] {
            let response = serve_request(app.clone(), request("GET", path, Value::Null)).await;
            assert_eq!(
                response.headers()[header::CONTENT_SECURITY_POLICY],
                content_security_policy::VALUE
            );
        }
    }
}
