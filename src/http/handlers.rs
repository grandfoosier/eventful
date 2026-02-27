use axum::{extract::Path, extract::State, http::StatusCode, response::IntoResponse, Json};
use std::sync::Arc;

use crate::IngestResult;
use crate::http::types::{EventIn, EventRecordOut};
use crate::service::IngestService;
use crate::store::MemoryStore;
// use crate::Telemetry;

pub struct HttpState {
    pub ingest: IngestService,
    pub store: MemoryStore,
    // pub telemetry: Telemetry,
}

pub async fn post_events(State(state): State<Arc<HttpState>>, Json(payload): Json<EventIn>) -> impl IntoResponse {
    let event = payload.into_domain();
    let (rec, result) = state.ingest.ingest(event).await;
    match result {  
        IngestResult::Ok(inserted) => (StatusCode::ACCEPTED, format!("event_id: {}, status: {}, attempts: {}, inserted: {}",
            rec.event.event_id, rec.status, rec.attempts, inserted)).into_response(),
        IngestResult::StoreError(_e) => (StatusCode::CONFLICT, format!("event already exists with different payload hash")).into_response(),
        IngestResult::QueueError(e) => (StatusCode::SERVICE_UNAVAILABLE, format!("queue error: {}", e)).into_response(),
        IngestResult::QueueFull => (StatusCode::TOO_MANY_REQUESTS, "queue is full").into_response(),
    }
}

pub async fn get_event(State(state): State<Arc<HttpState>>, Path(id): Path<String>) -> impl IntoResponse {
    match state.store.get(&id).await {
        Ok(rec) => (StatusCode::OK, Json(EventRecordOut::from(rec))).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

// pub async fn healthz(State(state): State<Arc<HttpState>>) -> impl IntoResponse {
//     let q = state.telemetry.queue_depth.get();
//     (StatusCode::OK, format!("ok - queue_depth={}", q))
// }

// pub async fn metrics(State(state): State<Arc<HttpState>>) -> impl IntoResponse {
//     let body = state.telemetry.gather();
//     (StatusCode::OK, body)
// }
