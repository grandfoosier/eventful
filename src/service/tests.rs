#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use tokio::sync::mpsc;

    use crate::domain::{Event, EventPayload, EventStatus, EventType};
    use crate::service::{IngestResult, IngestService};
    use crate::store::MemoryStore;
    use crate::Telemetry;

    fn create_test_event(event_id: &str) -> Event {
        Event {
            event_id: event_id.to_string(),
            event_type: EventType::UserLoginFailed,
            occurred_at: Utc::now(),
            payload: EventPayload(json!({"user_id": "test_user"})),
        }
    }

    fn create_test_service(queue_capacity: usize) -> (IngestService, mpsc::Receiver<String>) {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let (tx, rx) = mpsc::channel(queue_capacity);
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        let telemetry = Telemetry::new();
        let service = IngestService::new(store, tx, shutdown_tx, telemetry);
        (service, rx)
    }

    #[tokio::test]
    async fn test_ingest_new_event() {
        let (service, mut rx) = create_test_service(10);
        let event = create_test_event("event-1");

        let (record, result) = service.ingest(event.clone()).await;

        assert!(matches!(result, IngestResult::Ok(true))); // new event
        assert_eq!(record.event.event_id, "event-1");
        assert_eq!(record.status, EventStatus::Received);

        // Event should be enqueued
        let enqueued_id = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed");
        assert_eq!(enqueued_id, "event-1");
    }

    #[tokio::test]
    async fn test_ingest_duplicate_event() {
        let (service, mut rx) = create_test_service(10);
        let event = create_test_event("event-1");

        let (record1, result1) = service.ingest(event.clone()).await;
        let (record2, result2) = service.ingest(event).await;

        assert!(matches!(result1, IngestResult::Ok(true))); // first is new
        assert!(matches!(result2, IngestResult::Ok(false))); // second is duplicate
        assert_eq!(record1.event.event_id, record2.event.event_id);

        // Only one message should be enqueued
        let enqueued_id = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed");
        assert_eq!(enqueued_id, "event-1");

        // No second message
        assert!(tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_ingest_queue_full() {
        let (service, _rx) = create_test_service(1); // Small queue
        let event1 = create_test_event("event-1");
        let event2 = create_test_event("event-2");

        // First event should enqueue successfully
        let (_record1, result1) = service.ingest(event1).await;
        assert!(matches!(result1, IngestResult::Ok(true)));

        // Without consuming from rx, queue is now full (capacity 1, 1 message queued)
        // Try to ingest second event when queue is full
        let (_record2, result2) = service.ingest(event2).await;

        assert!(matches!(result2, IngestResult::QueueFull));
    }

    #[tokio::test]
    async fn test_ingest_hash_mismatch() {
        let (service, _rx) = create_test_service(10);
        let event1 = Event {
            event_id: "event-1".to_string(),
            event_type: EventType::UserLoginFailed,
            occurred_at: Utc::now(),
            payload: EventPayload(json!({"user_id": "user1"})),
        };
        let event2 = Event {
            event_id: "event-1".to_string(), // same ID
            event_type: EventType::UserLoginFailed,
            occurred_at: Utc::now(),
            payload: EventPayload(json!({"user_id": "user2"})), // different payload
        };

        let (_record1, result1) = service.ingest(event1).await;
        let (_record2, result2) = service.ingest(event2).await;

        assert!(matches!(result1, IngestResult::Ok(true)));
        assert!(matches!(result2, IngestResult::StoreError(_)));
    }

    #[tokio::test]
    async fn test_ingest_multiple_events_in_sequence() {
        let (service, mut rx) = create_test_service(10);

        for i in 1..=5 {
            let event = create_test_event(&format!("event-{}", i));
            let (_record, result) = service.ingest(event).await;
            assert!(matches!(result, IngestResult::Ok(true)));
        }

        // All events should be enqueued
        for i in 1..=5 {
            let enqueued_id = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
                .await
                .expect("timeout")
                .expect("channel closed");
            assert_eq!(enqueued_id, format!("event-{}", i));
        }
    }

    #[tokio::test]
    async fn test_reserve_and_schedule_noop_for_completed() {
        let (service, _rx) = create_test_service(10);
        let event = create_test_event("event-1");

        let (mut record, _) = service.store.insert_if_absent(event).await;
        record.status = EventStatus::Completed;

        service.store.update_record_for_test(record).await;

        let result = service.reserve_and_schedule("event-1".to_string(), false).await;

        assert!(matches!(result, IngestResult::Ok(false)));
    }

    #[tokio::test]
    async fn test_idempotent_ingestion() {
        let (service, mut rx) = create_test_service(10);
        let event = create_test_event("event-1");

        // Ingest same event multiple times
        let (_record1, result1) = service.ingest(event.clone()).await;
        let (_record2, result2) = service.ingest(event.clone()).await;
        let (_record3, result3) = service.ingest(event).await;

        assert!(matches!(result1, IngestResult::Ok(true))); // new
        assert!(matches!(result2, IngestResult::Ok(false))); // duplicate
        assert!(matches!(result3, IngestResult::Ok(false))); // duplicate

        // Only one message in queue
        let id = rx.recv().await.unwrap();
        assert_eq!(id, "event-1");
        assert!(tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_telemetry_metrics_updated() {
        let (service, _rx) = create_test_service(10);

        let event1 = create_test_event("event-1");
        let event2 = create_test_event("event-2");

        service.ingest(event1).await;
        service.ingest(event2.clone()).await;
        service.ingest(event2).await; // duplicate

        let ingested = service.telemetry.events_ingested.get();
        let deduped = service.telemetry.events_deduped.get();

        assert_eq!(ingested, 2);
        assert_eq!(deduped, 1);
    }
}
