use std::net::SocketAddr;
use std::time::Instant;

use axum::body::Body;
use axum::extract::MatchedPath;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use metrics_exporter_prometheus::PrometheusBuilder;

/// Outcome of a fallible operation, used to label metrics instead of raw
/// string literals at each call site (which invite typos that split a
/// metric into two accidental series instead of one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Error,
}

impl Outcome {
    fn label(self) -> &'static str {
        match self {
            Outcome::Success => "success",
            Outcome::Error => "error",
        }
    }
}

impl From<bool> for Outcome {
    /// Convenience for the common `result.is_ok()` call site.
    fn from(is_ok: bool) -> Self {
        if is_ok {
            Outcome::Success
        } else {
            Outcome::Error
        }
    }
}

/// Which Kafka consumer recorded an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumerKind {
    Broadcast,
    UserEvents,
}

impl ConsumerKind {
    fn label(self) -> &'static str {
        match self {
            ConsumerKind::Broadcast => "broadcast",
            ConsumerKind::UserEvents => "user_events",
        }
    }
}

/// Records a Kafka publish attempt (`kafka_messages_published_total`).
pub fn record_kafka_published(outcome: Outcome) {
    metrics::counter!("kafka_messages_published_total", "outcome" => outcome.label()).increment(1);
}

/// Records a Kafka consume attempt (`kafka_messages_consumed_total`).
pub fn record_kafka_consumed(consumer: ConsumerKind, outcome: Outcome) {
    metrics::counter!(
        "kafka_messages_consumed_total",
        "consumer" => consumer.label(),
        "outcome" => outcome.label(),
    )
    .increment(1);
}

/// Records a message sent to a dead-letter topic (`kafka_dlq_total`).
pub fn record_kafka_dlq(consumer: ConsumerKind) {
    metrics::counter!("kafka_dlq_total", "consumer" => consumer.label()).increment(1);
}

/// Records the current count of unpublished outbox rows (`outbox_pending`).
pub fn record_outbox_pending(count: i64) {
    metrics::gauge!("outbox_pending").set(count as f64);
}

/// Records the age in seconds of the oldest unpublished outbox row
/// (`outbox_oldest_pending_seconds`); zero when the outbox is empty.
pub fn record_outbox_oldest_pending_seconds(seconds: f64) {
    metrics::gauge!("outbox_oldest_pending_seconds").set(seconds);
}

/// Records a WebSocket connection being added to a registry (`ws_connections`).
pub fn record_ws_connection_opened() {
    metrics::gauge!("ws_connections").increment(1);
}

/// Records a WebSocket connection being removed from a registry (`ws_connections`).
pub fn record_ws_connection_closed() {
    metrics::gauge!("ws_connections").decrement(1);
}

/// Records `http_requests_total` and `http_request_duration_seconds` for
/// every request, labeled by method/matched-route-template/status so
/// distinct resource IDs (e.g. `/api/channels/{channel_id}`) don't explode
/// into separate time series.
///
/// Must be installed with `route_layer` (not `layer`): the matched-path
/// extension this reads is only present once routing has resolved to a
/// specific route, which happens *inside* what `layer` wraps.
pub async fn track_http_metrics(req: Request<Body>, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().to_string();
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());

    let response = next.run(req).await;

    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();
    let labels = [("method", method), ("path", path), ("status", status)];

    metrics::counter!("http_requests_total", &labels).increment(1);
    metrics::histogram!("http_request_duration_seconds", &labels).record(latency);

    response
}

/// Starts a Prometheus exporter serving `/metrics` on its own listener,
/// separate from the application's HTTP port — every role (api, gateway,
/// worker, ...) runs one of these regardless of whether it serves any other
/// HTTP routes.
pub fn install_prometheus_recorder(port: u16) -> Result<(), anyhow::Error> {
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    PrometheusBuilder::new()
        .with_http_listener(addr)
        .install()
        .map_err(|e| anyhow::anyhow!("failed to install Prometheus exporter: {e}"))
}
