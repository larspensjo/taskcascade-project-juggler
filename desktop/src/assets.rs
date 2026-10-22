use std::borrow::Cow;

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    response::IntoResponse,
    Router,
};

pub struct Asset {
    pub bytes: Cow<'static, [u8]>,
    pub content_type: String,
}

pub trait AssetSource: Clone + Send + Sync + 'static {
    fn get(&self, path: &str) -> Option<Asset>;
}

#[derive(rust_embed::RustEmbed, Clone)]
#[folder = "../frontend/dist"]
#[allow_missing = true]
pub struct FrontendDist;

impl AssetSource for FrontendDist {
    fn get(&self, path: &str) -> Option<Asset> {
        let file = Self::get(path)?;
        Some(Asset {
            content_type: file.metadata.mimetype().to_owned(),
            bytes: file.data,
        })
    }
}

/// Serve only literal file paths, without a client-side routing fallback.
pub fn router<S: AssetSource>(source: S) -> Router {
    Router::new().fallback(move |request: Request<Body>| {
        let source = source.clone();
        async move {
            if request.method() != Method::GET {
                return StatusCode::METHOD_NOT_ALLOWED.into_response();
            }
            let path = request.uri().path().trim_start_matches('/');
            // Built asset names are literal paths. Reject encoded/Windows paths
            // before a disk-backed source can interpret them as traversal.
            if path.contains(['%', '\\', ':', '\0'])
                || path.split('/').any(|part| matches!(part, "." | ".."))
            {
                return StatusCode::NOT_FOUND.into_response();
            }
            let path = if path.is_empty() { "index.html" } else { path };
            match source.get(path) {
                Some(asset) => {
                    ([(header::CONTENT_TYPE, asset.content_type)], asset.bytes).into_response()
                }
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use axum::{
        body::Body,
        http::{header, Method, Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::*;

    #[derive(Clone)]
    pub(crate) struct Fixture;

    impl AssetSource for Fixture {
        fn get(&self, path: &str) -> Option<Asset> {
            let (bytes, content_type): (&'static [u8], &str) = match path {
                "index.html" => (b"<html>TaskCascade</html>", "text/html"),
                "assets/x.js" | "assets/app.js" => (b"export {};", "text/javascript"),
                // Deliberately exposed by the fixture: the router must reject traversal.
                "../Cargo.toml" | "../../Cargo.toml" => (b"secret", "text/plain"),
                _ => return None,
            };
            Some(Asset {
                bytes: Cow::Borrowed(bytes),
                content_type: content_type.into(),
            })
        }
    }

    #[tokio::test]
    async fn serves_files_with_content_types_and_no_spa_fallback() {
        let app = router(Fixture);
        for (path, status, content_type) in [
            ("/", StatusCode::OK, Some("text/html")),
            ("/assets/x.js", StatusCode::OK, Some("text/javascript")),
            ("/missing", StatusCode::NOT_FOUND, None),
            ("/..%2F..%2FCargo.toml", StatusCode::NOT_FOUND, None),
            ("/../Cargo.toml", StatusCode::NOT_FOUND, None),
            ("/assets/..%5cCargo.toml", StatusCode::NOT_FOUND, None),
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{path}");
            if let Some(expected) = content_type {
                assert_eq!(response.headers()[header::CONTENT_TYPE], expected);
            }
        }
    }

    #[tokio::test]
    async fn rejects_non_get_methods_including_head() {
        for method in [
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::PATCH,
            Method::OPTIONS,
            Method::TRACE,
            Method::CONNECT,
        ] {
            let response = router(Fixture)
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri("/")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(matches!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_FOUND
            ));
        }
    }
}
