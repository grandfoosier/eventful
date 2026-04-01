#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use crate::domain::{Event, EventPayload, EventRecord, EventStatus, EventType};

    #[test]
    fn test_event_status_transitions() {
        // Valid transitions
        assert!(EventStatus::Received.can_transition(EventStatus::Processing));
        assert!(EventStatus::Processing.can_transition(EventStatus::Completed));
        assert!(EventStatus::Processing.can_transition(EventStatus::FailedRetry));
        assert!(EventStatus::Processing.can_transition(EventStatus::Failed));

        // Invalid transitions
        assert!(!EventStatus::Received.can_transition(EventStatus::Completed));
        assert!(!EventStatus::Received.can_transition(EventStatus::Failed));
        assert!(!EventStatus::Completed.can_transition(EventStatus::Processing));
        assert!(!EventStatus::Failed.can_transition(EventStatus::Processing));
        assert!(!EventStatus::FailedRetry.can_transition(EventStatus::Received));
    }

    #[test]
    fn test_event_record_creation() {
        let event = Event {
            event_id: "test-123".to_string(),
            event_type: EventType::UserLoginFailed,
            occurred_at: Utc::now(),
            payload: EventPayload(json!({"user_id": "user1"})),
        };

        let record = EventRecord::new(event.clone());

        assert_eq!(record.event.event_id, "test-123");
        assert_eq!(record.status, EventStatus::Received);
        assert!(!record.queued);
        assert_eq!(record.attempts, 0);
        assert_eq!(record.payload_hash, event.get_hash());
        assert!(record.queue_time.is_none());
        assert!(record.last_error.is_none());
        assert!(record.result.is_none());
    }

    #[test]
    fn test_event_hash_generation() {
        let payload = EventPayload(json!({"user_id": "user1", "action": "login"}));
        let event1 = Event {
            event_id: "event1".to_string(),
            event_type: EventType::UserLoginFailed,
            occurred_at: Utc::now(),
            payload: payload.clone(),
        };

        let event2 = Event {
            event_id: "event2".to_string(),
            event_type: EventType::UserLoginFailed,
            occurred_at: Utc::now(),
            payload: payload.clone(),
        };

        // Same payload should produce same hash
        assert_eq!(event1.get_hash(), event2.get_hash());
    }

    #[test]
    fn test_event_hash_differs_for_different_payloads() {
        let event1 = Event {
            event_id: "event1".to_string(),
            event_type: EventType::UserLoginFailed,
            occurred_at: Utc::now(),
            payload: EventPayload(json!({"user_id": "user1"})),
        };

        let event2 = Event {
            event_id: "event1".to_string(),
            event_type: EventType::UserLoginFailed,
            occurred_at: Utc::now(),
            payload: EventPayload(json!({"user_id": "user2"})),
        };

        // Different payloads should produce different hashes
        assert_ne!(event1.get_hash(), event2.get_hash());
    }

    #[test]
    fn test_event_type_conversions() {
        assert_eq!(
            String::from(EventType::UserLoginFailed),
            "user.login_failed"
        );
        assert_eq!(
            String::from(EventType::ConnectionTimeout),
            "connection.timeout"
        );
        assert_eq!(
            String::from(EventType::DataProcessingError),
            "data.processing_error"
        );

        let custom = EventType::Other("custom.event".to_string());
        assert_eq!(String::from(custom), "custom.event");
    }

    #[test]
    fn test_event_type_string_parsing() {
        let event_type: EventType = "user.login_failed".to_string().try_into().unwrap();
        assert!(matches!(event_type, EventType::UserLoginFailed));

        let event_type: EventType = "connection.timeout".to_string().try_into().unwrap();
        assert!(matches!(event_type, EventType::ConnectionTimeout));

        let event_type: EventType = "custom.event".to_string().try_into().unwrap();
        assert!(matches!(event_type, EventType::Other(ref s) if s == "custom.event"));
    }

    #[test]
    fn test_event_status_display() {
        assert_eq!(format!("{}", EventStatus::Received), "Received");
        assert_eq!(format!("{}", EventStatus::Processing), "Processing");
        assert_eq!(format!("{}", EventStatus::Completed), "Completed");
        assert_eq!(format!("{}", EventStatus::FailedRetry), "FailedRetry");
        assert_eq!(format!("{}", EventStatus::Failed), "Failed");
    }
}
