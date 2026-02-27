use chrono::{DateTime, Duration, TimeDelta, Utc};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::domain::{Event, EventRecord, EventStatus};

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("not found")]
    NotFound,
    #[error("invalid state transition")]
    InvalidStateTransition,
    #[error("hash mismatch")]
    HashMismatch,
    #[error("conflict")]
    Conflict,
}

pub enum ReserveResult {
    Noop,
    Enqueue,
}

pub enum StartResult {
    Start { attempt: u32, record: EventRecord },
    SkipAlreadyCompleted,
    SkipNotRunnable,
    NotFound,
}

#[derive(Clone)]
pub struct MemoryStore {
    inner: Arc<Mutex<HashMap<String, EventRecord>>>,
    base_backoff_ms: u64,
    max_retries: u32,
    queue_stale_time_ms: u64,
    burst_size: usize,
}

impl MemoryStore {
    pub fn new(
        base_backoff_ms: u64, 
        max_retries: u32, 
        queue_stale_time_ms: u64, 
        burst_size: usize
    ) -> Self { Self {
        inner: Arc::new(Mutex::new(HashMap::new())),
        base_backoff_ms,
        max_retries,
        queue_stale_time_ms,
        burst_size,
    }}

    /// Insert if absent. Returns result, for ok: true if inserted, false if already existed.
    pub async fn insert_if_absent(&self, event: Event) -> (EventRecord, Result<bool, StoreError>) {
        let mut map = self.inner.lock().await;
        if let Some(rec) = map.get(&event.event_id) {
            if rec.payload_hash != event.get_hash() {
                // This means the same event_id is being reused with different payload, which should not happen.
                // We treat it as an error. In a real implementation, you might want to handle this differently.
                return (rec.clone(), Err(StoreError::HashMismatch));
            }
            (rec.clone(), Ok(false)) // already exists, return existing record
        } else {
            let record = EventRecord::new(event);
            map.insert(record.event.event_id.clone(), record.clone());
            (record, Ok(true))
        }
    }

    pub async fn get(&self, id: &str) -> Result<EventRecord, StoreError> {
        let map = self.inner.lock().await;
        map.get(id).cloned().ok_or(StoreError::NotFound)
    }

    pub async fn list_enqueue_candidates(&self, now: DateTime<Utc>) -> Vec<String> {
        let map = self.inner.lock().await;
        map.values()
           .filter(|rec| (rec.status == EventStatus::Received || 
                          // Times shouldn't be missing, but if they are, we treat it as ready for retry or requeue to avoid leaving it unprocessed indefinitely.
                          (rec.status == EventStatus::FailedRetry && rec.retry_time.map_or(true, |rt| rt <= now))
                         ) && (!rec.queued || rec.queue_time.map_or(true, |qt| (now - qt) > Duration::milliseconds(self.queue_stale_time_ms as i64))))
           .take(self.burst_size) // batch size to avoid returning too many at once, in a real implementation we would want proper pagination
           .map(|rec| rec.event.event_id.clone())   
           .collect()
    }

    pub async fn reserve_enqueue_if_needed(&self, event_id: &str, now: DateTime<Utc>) -> ReserveResult {
        let mut map = self.inner.lock().await;
        let rec = match map.get_mut(event_id) {
            Some(rec) => rec,
            None => return ReserveResult::Noop, // event_id not found
        };
        let runnable = rec.status == EventStatus::Received || 
            (rec.status == EventStatus::FailedRetry && rec.retry_time.map_or(true, |rt| rt <= now));
        let enqueueable = !rec.queued || rec.queue_time.map_or(true, |qt| (now - qt) > Duration::milliseconds(self.queue_stale_time_ms as i64));
        if runnable && enqueueable {
            rec.queued = true;
            rec.queue_time = Some(now);
            rec.updated_at = now;
            ReserveResult::Enqueue
        } else {
            ReserveResult::Noop
        }
    }

    /// Claim for processing: move Received -> Processing and increment attempts atomically.
    pub async fn try_start_processing(&self, id: &str) -> StartResult {
        let mut map = self.inner.lock().await;
        if let Some(rec) =  map.get_mut(id) {
            if rec.status == EventStatus::Completed {
                StartResult::SkipAlreadyCompleted
            } else if rec.status == EventStatus::Received || 
                (rec.status == EventStatus::FailedRetry && rec.retry_time.map_or(false, |rt| rt <= Utc::now()))
            {
                rec.status = EventStatus::Processing;
                rec.attempts = rec.attempts.saturating_add(1);
                rec.updated_at = Utc::now();
                rec.queued = false; // clear queued state when claiming for processing, so it can be re-queued if processing fails
                rec.queue_time = None;
                StartResult::Start { attempt: rec.attempts, record: rec.clone() }
            } else { StartResult::SkipNotRunnable }
        }
        else { StartResult::NotFound }
    }

    pub async fn finish_processing(&self, id: &str, attempt: u32, result: Result<(), String>) -> Result<(), StoreError> {
        let mut map = self.inner.lock().await;
        let rec = map.get_mut(id).ok_or(StoreError::NotFound)?;
        if rec.status != EventStatus::Processing {
            return Err(StoreError::InvalidStateTransition);
        }
        if rec.attempts != attempt {
            // This means the record has been retried and claimed by another worker since this worker claimed it, 
            // which should not happen because we clear queued state when claiming, 
            // but we treat it as a conflict and do not update the record in this case to avoid accidentally overwriting the state of the new worker.
            return Err(StoreError::Conflict);
        }
        match result {
            Ok(()) => {
                rec.result = Some(Value::String("success".into()));
                rec.status = EventStatus::Completed;
            },
            Err(e) => {
                if e == "Shutting down" {
                    // If the failure is due to shutdown, we want to retry immediately when the service restarts, so we set retry_time to now.
                    rec.status = EventStatus::FailedRetry;
                    rec.retry_time = Some(Utc::now());
                } else {
                    let retries_used = rec.attempts - 1; // attempts is incremented when claiming, so subtract 1 to get retries used
                    if retries_used >= self.max_retries {
                        rec.status = EventStatus::Failed;
                    } else {
                        rec.status = EventStatus::FailedRetry;
                        rec.retry_time = // exponential backoff: base_backoff_ms * 2^(retries_used), max of 30 seconds to avoid excessively long wait time in this simple implementation
                            Some(Utc::now() + Duration::milliseconds((self.base_backoff_ms as u64 * 2u64.pow(retries_used)) as i64).min(TimeDelta::seconds(30)));
                    }
                }
                rec.last_error = Some(e);
            }
        }
        rec.updated_at = Utc::now();
        Ok(())
    }
}