use axum::body::{to_bytes, Body};
use axum::Router;
use http::{header, HeaderValue, StatusCode, Uri};
use http::{Request, Response};
use tower::ServiceExt;

use crate::content_security_policy;

pub const SCHEME: &str = "taskcascade";
pub const PAGE_ORIGIN: &str = "http://taskcascade.localhost";

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
    use axum::{
        body::{to_bytes, Body},
        extract::Query,
        routing::get,
    };
    use http::{header, StatusCode};
    use serde_json::{json, Value};
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
        let bridged = compose(state, Fixture);
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
        let app = compose(state, Fixture);
        for path in ["/", "/assets/app.js", "/api/health", "/missing"] {
            let response = serve_request(app.clone(), request("GET", path, Value::Null)).await;
            assert_eq!(
                response.headers()[header::CONTENT_SECURITY_POLICY],
                content_security_policy::VALUE
            );
        }
    }
}
