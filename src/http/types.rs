use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{event::{Event, EventPayload, EventType}, state::EventStatus};

#[derive(Clone, Debug, Deserialize)]
pub struct EventIn {
    pub event_id: String,
    pub event_type: String,
    pub occurred_at: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Serialize)]
pub struct EventRecordOut {
    pub event: Event,
    pub payload_hash: String,
    pub status: EventStatus,
    pub queued: bool,
    pub queue_time: Option<DateTime<Utc>>,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub retry_time: Option<DateTime<Utc>>,
    pub result: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<crate::domain::event::EventRecord> for EventRecordOut {
    fn from(rec: crate::domain::event::EventRecord) -> Self {
        Self {
            event: rec.event,
            payload_hash: rec.payload_hash,
            status: rec.status,
            queued: rec.queued,
            queue_time: rec.queue_time,
            attempts: rec.attempts,
            last_error: rec.last_error,
            retry_time: rec.retry_time,
            result: rec.result,
            created_at: rec.created_at,
            updated_at: rec.updated_at,
        }
    }
}

impl EventIn {
    pub fn into_domain(self) -> crate::domain::event::Event {
        let s = self.event_type.clone();
        let et = EventType::try_from(s.clone()).unwrap_or(EventType::Other(s));
        crate::domain::event::Event { event_id: self.event_id, event_type: et, occurred_at: self.occurred_at, payload: EventPayload(self.payload) }
    }
}
