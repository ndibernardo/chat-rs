use std::time::Duration;

use axum::body::Body;
use axum::http::Request;
use axum::http::Response;
use axum::Router;
use tower_http::trace::TraceLayer;
use tracing::Span;

/// Applies the standard request-tracing layer used by both services.
///
/// Logs the path only, never the query string or headers: an `Authorization`
/// header or a `?token=...` query parameter both carry credentials, so
/// logging either would put them in logs/proxies.
pub fn with_request_trace<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &Request<Body>| {
            tracing::info_span!(
                "http_request",
                method = %request.method(),
                path = %request.uri().path(),
                version = ?request.version(),
            )
        })
        .on_request(|request: &Request<Body>, _span: &Span| {
            tracing::info!(
                method = %request.method(),
                path = %request.uri().path(),
                "Request started"
            );
        })
        .on_response(
            |response: &Response<Body>, latency: Duration, _span: &Span| {
                tracing::info!(
                    status = response.status().as_u16(),
                    latency_ms = latency.as_millis(),
                    "Request completed"
                );
            },
        );

    router.layer(trace_layer)
}
