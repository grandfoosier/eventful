#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use crate::domain::{Event, EventPayload, EventStatus, EventType};
    use crate::store::{MemoryStore, ReserveResult, StartResult, StoreError};

    fn create_test_event(event_id: &str) -> Event {
        Event {
            event_id: event_id.to_string(),
            event_type: EventType::UserLoginFailed,
            occurred_at: Utc::now(),
            payload: EventPayload(json!({"user_id": "test_user"})),
        }
    }

    #[tokio::test]
    async fn test_insert_if_absent_new_event() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let event = create_test_event("event-1");

        let (record, result) = store.insert_if_absent(event.clone()).await;

        assert!(result.is_ok());
        assert!(result.unwrap() == true); // newly inserted
        assert_eq!(record.event.event_id, "event-1");
        assert_eq!(record.status, EventStatus::Received);
        assert_eq!(record.payload_hash, event.get_hash());
    }

    #[tokio::test]
    async fn test_insert_if_absent_duplicate_event() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let event = create_test_event("event-1");

        let (_, result1) = store.insert_if_absent(event.clone()).await;
        let (record2, result2) = store.insert_if_absent(event.clone()).await;

        assert!(result1.unwrap() == true); // first insert is true
        assert!(result2.unwrap() == false); // second insert returns false (already exists)
        assert_eq!(record2.event.event_id, "event-1");
    }

    #[tokio::test]
    async fn test_insert_if_absent_hash_mismatch() {
        let store = MemoryStore::new(100, 5, 3000, 128);
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

        let (_, result1) = store.insert_if_absent(event1).await;
        let (_, result2) = store.insert_if_absent(event2).await;

        assert!(result1.is_ok());
        assert!(result2.is_err());
        assert!(matches!(result2, Err(StoreError::HashMismatch)));
    }

    #[tokio::test]
    async fn test_get_event() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let event = create_test_event("event-1");

        _ = store.insert_if_absent(event).await;
        let result = store.get("event-1").await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().event.event_id, "event-1");
    }

    #[tokio::test]
    async fn test_get_nonexistent_event() {
        let store = MemoryStore::new(100, 5, 3000, 128);

        let result = store.get("nonexistent").await;

        assert!(result.is_err());
        assert!(matches!(result, Err(StoreError::NotFound)));
    }

    #[tokio::test]
    async fn test_reserve_enqueue_if_needed_noop_already_completed() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let event = create_test_event("event-1");

        let (mut record, _) = store.insert_if_absent(event).await;
        record.status = EventStatus::Completed;

        store.update_record_for_test(record).await;

        let result = store.reserve_enqueue_if_needed("event-1", Utc::now()).await;
        assert!(matches!(result, ReserveResult::Noop));
    }

    #[tokio::test]
    async fn test_reserve_enqueue_if_needed_enqueue_received() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let event = create_test_event("event-1");

        _ = store.insert_if_absent(event).await;

        let result = store.reserve_enqueue_if_needed("event-1", Utc::now()).await;

        assert!(matches!(result, ReserveResult::Enqueue));

        let record = store.get("event-1").await.unwrap();
        assert!(record.queued);
        assert!(record.queue_time.is_some());
    }

    #[tokio::test]
    async fn test_reserve_enqueue_if_needed_idempotent() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let event = create_test_event("event-1");

        _ = store.insert_if_absent(event).await;

        let result1 = store.reserve_enqueue_if_needed("event-1", Utc::now()).await;
        let result2 = store.reserve_enqueue_if_needed("event-1", Utc::now()).await;

        assert!(matches!(result1, ReserveResult::Enqueue));
        assert!(matches!(result2, ReserveResult::Noop)); // Already queued, so noop

        let record = store.get("event-1").await.unwrap();
        assert!(record.queued);
    }

    #[tokio::test]
    async fn test_try_start_processing() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let event = create_test_event("event-1");

        _ = store.insert_if_absent(event).await;

        let result = store.try_start_processing("event-1").await;

        match result {
            StartResult::Start { attempt, record } => {
                assert_eq!(attempt, 1);
                assert_eq!(record.status, EventStatus::Processing);
                assert!(!record.queued);
            }
            _ => panic!("Expected Start result"),
        }

        // Verify the record was updated
        let record = store.get("event-1").await.unwrap();
        assert_eq!(record.status, EventStatus::Processing);
        assert_eq!(record.attempts, 1);
    }

    #[tokio::test]
    async fn test_try_start_processing_already_completed() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let event = create_test_event("event-1");

        let (mut record, _) = store.insert_if_absent(event).await;
        record.status = EventStatus::Completed;

        store.update_record_for_test(record).await;

        let result = store.try_start_processing("event-1").await;

        assert!(matches!(result, StartResult::SkipAlreadyCompleted));
    }

    #[tokio::test]
    async fn test_try_start_processing_not_found() {
        let store = MemoryStore::new(100, 5, 3000, 128);

        let result = store.try_start_processing("nonexistent").await;

        assert!(matches!(result, StartResult::NotFound));
    }

    #[tokio::test]
    async fn test_finish_processing_success() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let event = create_test_event("event-1");

        _ = store.insert_if_absent(event).await;
        let start_result = store.try_start_processing("event-1").await;

        let (attempt, _) = match start_result {
            StartResult::Start { attempt, record: _ } => (attempt, ""),
            _ => panic!("Expected Start result"),
        };

        let finish_result = store.finish_processing("event-1", attempt, Ok(())).await;

        assert!(finish_result.is_ok());

        let record = store.get("event-1").await.unwrap();
        assert_eq!(record.status, EventStatus::Completed);
        assert!(record.result.is_some());
    }

    #[tokio::test]
    async fn test_finish_processing_failure_with_retry() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let event = create_test_event("event-1");

        _ = store.insert_if_absent(event).await;
        let start_result = store.try_start_processing("event-1").await;

        let attempt = match start_result {
            StartResult::Start { attempt, record: _ } => attempt,
            _ => panic!("Expected Start result"),
        };

        let finish_result = store.finish_processing("event-1", attempt, Err("processing failed".to_string())).await;

        assert!(finish_result.is_ok());

        let record = store.get("event-1").await.unwrap();
        assert_eq!(record.status, EventStatus::FailedRetry);
        assert!(record.last_error.is_some());
        assert!(record.retry_time.is_some());
    }

    #[tokio::test]
    async fn test_finish_processing_max_retries_exceeded() {
        let store = MemoryStore::new(100, 2, 3000, 128); // max_retries = 2
        let event = create_test_event("event-1");

        _ = store.insert_if_absent(event).await;

        // Process and fail 3 times (with sleep between attempts for exponential backoff)
        for i in 1..=3 {
            let start_result = store.try_start_processing("event-1").await;
            let attempt = match start_result {
                StartResult::Start { attempt, record: _ } => attempt,
                _ => panic!("Expected Start result on iteration {}", i),
            };

            let _ = store.finish_processing("event-1", attempt, Err("failed".to_string())).await;
            
            // Sleep to let exponential backoff time pass before next retry
            // Backoff times: 100ms, 200ms, 400ms. Sleep 500ms to cover all.
            if i < 3 {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }

        let record = store.get("event-1").await.unwrap();
        assert_eq!(record.status, EventStatus::Failed); // Should be Failed after max retries
    }

    #[tokio::test]
    async fn test_finish_processing_shutdown_failure() {
        let store = MemoryStore::new(100, 5, 3000, 128);
        let event = create_test_event("event-1");

        _ = store.insert_if_absent(event).await;
        let start_result = store.try_start_processing("event-1").await;

        let attempt = match start_result {
            StartResult::Start { attempt, record: _ } => attempt,
            _ => panic!("Expected Start result"),
        };

        let finish_result = store.finish_processing("event-1", attempt, Err("Shutting down".to_string())).await;

        assert!(finish_result.is_ok());

        let record = store.get("event-1").await.unwrap();
        assert_eq!(record.status, EventStatus::FailedRetry);
        assert_eq!(record.last_error.as_deref(), Some("Shutting down"));
    }

    #[tokio::test]
    async fn test_list_enqueue_candidates() {
        let store = MemoryStore::new(100, 5, 3000, 128);

        // Insert three events
        for i in 1..=3 {
            let event = create_test_event(&format!("event-{}", i));
            _ = store.insert_if_absent(event).await;
        }

        let now = Utc::now();
        let candidates = store.list_enqueue_candidates(now).await;

        assert_eq!(candidates.len(), 3);
    }

    #[tokio::test]
    async fn test_list_enqueue_candidates_excludes_completed() {
        let store = MemoryStore::new(100, 5, 3000, 128);

        let event1 = create_test_event("event-1");
        let event2 = create_test_event("event-2");

        let (mut record1, _) = store.insert_if_absent(event1).await;
        _ = store.insert_if_absent(event2).await;

        record1.status = EventStatus::Completed;
        store.update_record_for_test(record1).await;

        let now = Utc::now();
        let candidates = store.list_enqueue_candidates(now).await;

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0], "event-2");
    }

    #[tokio::test]
    async fn test_list_enqueue_candidates_respects_stale_time() {
        let store = MemoryStore::new(100, 5, 100, 128); // 100ms stale time
        let event = create_test_event("event-1");

        _ = store.insert_if_absent(event).await;
        store.reserve_enqueue_if_needed("event-1", Utc::now()).await;

        let now = Utc::now();
        let candidates = store.list_enqueue_candidates(now).await;
        assert_eq!(candidates.len(), 0); // Should not be re-queued immediately

        // Wait past stale time and try again
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
        let now = Utc::now();
        let candidates = store.list_enqueue_candidates(now).await;
        assert_eq!(candidates.len(), 1); // Should be available now
    }

    #[tokio::test]
    async fn test_queued_count_tracking() {
        let store = MemoryStore::new(100, 5, 3000, 128);

        let event1 = create_test_event("event-1");
        let event2 = create_test_event("event-2");

        _ = store.insert_if_absent(event1).await;
        _ = store.insert_if_absent(event2).await;

        assert_eq!(store.queued_count(), 0);

        store.reserve_enqueue_if_needed("event-1", Utc::now()).await;
        assert_eq!(store.queued_count(), 1);

        store.reserve_enqueue_if_needed("event-2", Utc::now()).await;
        assert_eq!(store.queued_count(), 2);

        // Processing should decrease queued count
        store.try_start_processing("event-1").await;
        assert_eq!(store.queued_count(), 1);
    }

    #[tokio::test]
    async fn test_exponential_backoff_retry_time() {
        let store = MemoryStore::new(100, 10, 3000, 128);
        let event = create_test_event("event-1");

        _ = store.insert_if_absent(event).await;

        // Attempt 1
        store.try_start_processing("event-1").await;
        store.finish_processing("event-1", 1, Err("failed".to_string())).await.ok();

        let record1 = store.get("event-1").await.unwrap();
        let retry_time1 = record1.retry_time.unwrap();

        // Sleep to let exponential backoff time pass
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // Attempt 2
        store.reserve_enqueue_if_needed("event-1", Utc::now()).await;
        store.try_start_processing("event-1").await;
        store.finish_processing("event-1", 2, Err("failed".to_string())).await.ok();

        let record2 = store.get("event-1").await.unwrap();
        let retry_time2 = record2.retry_time.unwrap();

        // Second retry should have larger backoff than first
        // Attempt 1 backoff: 100 * 2^0 = 100ms
        // Attempt 2 backoff: 100 * 2^1 = 200ms
        assert!(retry_time2 > retry_time1);
    }
}
