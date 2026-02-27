use chrono::{DateTime, Utc};
use md5::compute;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::state::EventStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: String,
    pub event_type: EventType,
    pub occurred_at: DateTime<Utc>,
    pub payload: EventPayload,
}

impl Event {
    pub fn get_hash(&self) -> String {
        // For simplicity, we hash the payload JSON string. In a real implementation,
        // you might want to include more fields or use a more robust hashing strategy.
        let payload_str = serde_json::to_string(&self.payload).unwrap_or_default();
        format!("{:x}", compute(payload_str))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    #[serde(rename = "user.login_failed")]
    UserLoginFailed,
    #[serde(rename = "connection.timeout")]
    ConnectionTimeout,
    #[serde(rename = "data.processing_error")]
    DataProcessingError,
    // add more event types as needed
    Other(String),
}

impl From<EventType> for String {
    fn from(et: EventType) -> Self {
        match et {
            EventType::UserLoginFailed => "user.login_failed".to_string(),
            EventType::ConnectionTimeout => "connection.timeout".to_string(),
            EventType::DataProcessingError => "data.processing_error".to_string(),
            EventType::Other(s) => s,
        }
    }
}

impl TryFrom<String> for EventType {
    type Error = ();
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "user.login_failed" => Ok(EventType::UserLoginFailed),
            "connection.timeout" => Ok(EventType::ConnectionTimeout),
            "data.processing_error" => Ok(EventType::DataProcessingError),
            other => Ok(EventType::Other(other.to_string())),
        }
    }
}

/// Simple wrapper for the payload. Keep it extensible; for now we store raw
/// JSON so processors can interpret it according to `EventType`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventPayload(pub Value);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
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

impl EventRecord {
    pub fn new(event: Event) -> Self {
        let now = Utc::now();
        let payload_hash = event.get_hash();
        Self {
            event,
            payload_hash,
            status: EventStatus::Received,
            queued: false,
            queue_time: None,
            attempts: 0,
            last_error: None,
            retry_time: None,
            result: None,
            created_at: now,
            updated_at: now,
        }
    }
}