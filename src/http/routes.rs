use crate::http::handlers::{get_event, healthz, metrics, post_events, HttpState};
use axum::{routing::get, routing::post, Router};

pub fn router(state: std::sync::Arc<HttpState>) -> Router {
    Router::new()
        .route("/events", post(post_events))
        .route("/events/:id", get(get_event))
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics))
        .with_state(state)
}

// The router is returned so the caller can run the server and control graceful
// shutdown. Using the router directly avoids re-export mismatches for `Server`.
pub fn build_router(state: std::sync::Arc<HttpState>) -> Router {
    router(state)
}
