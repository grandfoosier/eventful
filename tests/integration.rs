// Integration tests for the full event processing pipeline
#[cfg(test)]
mod integration_tests {
    use chrono::Utc;
    use serde_json::json;
    use tokio::sync::mpsc;

    use eventful::domain::{Event, EventPayload, EventStatus, EventType};
    use eventful::service::{dispatch_loop, IngestService, IngestResult};
    use eventful::store::{MemoryStore, StartResult};
    use eventful::Telemetry;

    fn create_test_event(event_id: &str) -> Event {
        Event {
            event_id: event_id.to_string(),
            event_type: EventType::UserLoginFailed,
            occurred_at: Utc::now(),
            payload: EventPayload(json!({"user_id": "test_user"})),
        }
    }

    #[tokio::test]
    async fn test_event_ingestion_and_processing() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let (tx, rx) = mpsc::channel(100);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let telemetry = Telemetry::new();

        let ingest = IngestService::new(store.clone(), tx, shutdown_tx.clone(), telemetry.clone());

        // Spawn dispatch loop
        let store_clone = store.clone();
        let dispatch_handle = tokio::spawn(async move {
            dispatch_loop(
                store_clone,
                rx,
                2, // 2 concurrent workers
                5000,
                shutdown_rx,
                telemetry,
            )
            .await;
        });

        // Ingest an event
        let event = create_test_event("event-1");
        let (record, result) = ingest.ingest(event).await;

        assert!(matches!(result, IngestResult::Ok(_)));
        assert_eq!(record.status, EventStatus::Received);

        // Give processing loop time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Check that event was processed
        let processed_record = store.get("event-1").await.unwrap();
        assert!(
            processed_record.status == EventStatus::Completed
                || (processed_record.status == EventStatus::Processing),
            "Event status: {:?}",
            processed_record.status
        );

        // Shutdown
        shutdown_tx.send(true).ok();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        dispatch_handle.abort();
    }

    #[tokio::test]
    async fn test_event_deduplication_across_ingest() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let (tx, _rx) = mpsc::channel(100);
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        let telemetry = Telemetry::new();

        let ingest = IngestService::new(store.clone(), tx, shutdown_tx, telemetry.clone());

        let event = create_test_event("event-1");

        // Ingest same event 3 times
        let (record1, result1) = ingest.ingest(event.clone()).await;
        let (record2, result2) = ingest.ingest(event.clone()).await;
        let (record3, result3) = ingest.ingest(event).await;

        // First ingestion is new
        let is_new1 = matches!(result1, IngestResult::Ok(true));
        assert!(is_new1);

        // Subsequent ingestions are duplicates
        let is_new2 = matches!(result2, IngestResult::Ok(false));
        let is_new3 = matches!(result3, IngestResult::Ok(false));
        assert!(is_new2);
        assert!(is_new3);

        // All should return same record
        assert_eq!(record1.event.event_id, record2.event.event_id);
        assert_eq!(record2.event.event_id, record3.event.event_id);
    }

    #[tokio::test]
    async fn test_concurrent_event_ingestion() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let (tx, _rx) = mpsc::channel(1000);
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        let telemetry = Telemetry::new();

        let ingest = IngestService::new(store.clone(), tx, shutdown_tx, telemetry);

        let mut handles = vec![];

        // Spawn multiple tasks ingesting events
        for i in 0..10 {
            let ingest_clone = ingest.clone();
            let handle = tokio::spawn(async move {
                let event = create_test_event(&format!("event-{}", i));
                ingest_clone.ingest(event).await
            });
            handles.push(handle);
        }

        // Wait for all to complete
        let results: Vec<_> = futures::future::join_all(handles)
            .await
            .into_iter()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(results.len(), 10);

        // Verify all events are in store
        for i in 0..10 {
            let record = store.get(&format!("event-{}", i)).await;
            assert!(record.is_ok());
        }
    }

    #[tokio::test]
    async fn test_retry_mechanism() {
        let store = MemoryStore::new(100, 3, 3000, 128);
        let event = create_test_event("event-1");

        _ = store.insert_if_absent(event).await;

        // First attempt
        let start1 = store.try_start_processing("event-1").await;
        assert!(matches!(
            start1,
            StartResult::Start { .. }
        ));

        let attempt1 = match start1 {
            StartResult::Start { attempt, .. } => attempt,
            _ => panic!(),
        };

        // Fail processing
        store
            .finish_processing("event-1", attempt1, Err("failed".to_string()))
            .await
            .ok();

        let record1 = store.get("event-1").await.unwrap();
        assert_eq!(record1.status, EventStatus::FailedRetry);
        assert_eq!(record1.attempts, 1);

        // Sleep to let exponential backoff time pass
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // Re-queue for retry
        store
            .reserve_enqueue_if_needed("event-1", Utc::now())
            .await;

        // Second attempt
        let start2 = store.try_start_processing("event-1").await;
        assert!(matches!(
            start2,
            StartResult::Start { .. }
        ));

        let attempt2 = match start2 {
            StartResult::Start { attempt, .. } => attempt,
            _ => panic!(),
        };

        assert_eq!(attempt2, 2);

        // Complete successfully
        store
            .finish_processing("event-1", attempt2, Ok(()))
            .await
            .ok();

        let record_final = store.get("event-1").await.unwrap();
        assert_eq!(record_final.status, EventStatus::Completed);
        assert_eq!(record_final.attempts, 2);
    }

    #[tokio::test]
    async fn test_burst_size_limiting() {
        let store = MemoryStore::new(100, 5, 3000, 2); // burst_size = 2

        // Insert 5 events
        for i in 1..=5 {
            let event = create_test_event(&format!("event-{}", i));
            _ = store.insert_if_absent(event).await;
        }

        let now = Utc::now();
        let candidates = store.list_enqueue_candidates(now).await;

        // Should only return burst_size events
        assert_eq!(candidates.len(), 2);
    }

    #[tokio::test]
    async fn test_queue_and_processing_workflow() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let (tx, mut rx) = mpsc::channel(100);
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        let telemetry = Telemetry::new();

        let ingest = IngestService::new(store.clone(), tx, shutdown_tx, telemetry);

        // Ingest event
        let event = create_test_event("event-1");
        let (ingest_record, _) = ingest.ingest(event).await;

        println!("After ingest: status={:?}, queued={}", ingest_record.status, ingest_record.queued);
        assert_eq!(ingest_record.status, EventStatus::Received, "Ingest status should be Received");
        assert!(ingest_record.queued, "Ingest record should be queued=true, was {}", ingest_record.queued);

        // Verify it's in the queue
        let event_id = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("timeout")
            .expect("no event in queue");
        assert_eq!(event_id, "event-1");

        // Start processing
        let start_result = store.try_start_processing("event-1").await;
        assert!(matches!(start_result, StartResult::Start { .. }), "try_start_processing should return Start");

        let processing_record = store.get("event-1").await.unwrap();
        println!("After try_start_processing: status={:?}, queued={}", processing_record.status, processing_record.queued);
        assert_eq!(processing_record.status, EventStatus::Processing, "Processing status should be Processing, was {:?}", processing_record.status);
        assert!(!processing_record.queued, "Processing record should have queued=false, was {}", processing_record.queued);

        // Finish processing
        let attempt = match start_result {
            StartResult::Start { attempt, .. } => attempt,
            _ => panic!(),
        };
        store
            .finish_processing("event-1", attempt, Ok(()))
            .await
            .ok();

        let record = store.get("event-1").await.unwrap();
        assert_eq!(record.status, EventStatus::Completed);
    }

    #[tokio::test]
    async fn test_smoke() {
        // Simple smoke test: ingest an event and verify it's queued
        let store = MemoryStore::new(100, 5, 3000, 128);
        let (tx, _rx) = mpsc::channel(100);
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        let telemetry = Telemetry::new();

        let ingest = IngestService::new(store.clone(), tx, shutdown_tx, telemetry);
        let event = create_test_event("smoke-test-1");
        let (record, result) = ingest.ingest(event).await;

        assert!(matches!(result, IngestResult::Ok(true)));
        assert_eq!(record.status, EventStatus::Received);
        assert!(record.queued);
    }
}
