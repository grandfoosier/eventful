use chrono::Utc;
use tokio::sync::{mpsc, mpsc::error::TrySendError, watch};

use crate::ReserveResult;
use crate::domain::{Event, EventRecord};
use crate::store::MemoryStore;
use crate::Telemetry;

pub enum IngestResult {
    QueueError(String),
    QueueFull,
    StoreError(String),
    Ok(bool), // record and whether it's newly inserted
}

#[derive(Clone)]
pub struct IngestService {
    pub store: MemoryStore,
    pub tx: mpsc::Sender<String>,
    pub shutdown_tx: watch::Sender<bool>,
    pub telemetry: Telemetry,
}

impl IngestService {
    pub fn new(
        store: MemoryStore, 
        tx: mpsc::Sender<String>, 
        shutdown_tx: watch::Sender<bool>,
        telemetry: Telemetry
    ) -> Self {
        Self { store, tx, shutdown_tx, telemetry }
    }

    pub async fn ingest(&self, event: Event) -> (EventRecord, IngestResult) {
        // Try to insert into store. If it already exists, it's not an error, just return existing record.
        let (mut record, insert_result) = self.store.insert_if_absent(event).await;
        let inserted = match insert_result {
            Ok(inserted) => inserted,
            Err(e) => { // This means there is a hash mismatch for the same event_id, which should not happen. We treat it as an error, but still return the existing record for visibility.
                tracing::error!(event_id = %record.event.event_id, error = %e, "Failed to insert event");
                return (record, IngestResult::StoreError(e.to_string()));
            }
        };
        if inserted { self.telemetry.events_ingested.inc(); }
        else { self.telemetry.events_deduped.inc(); }
        let event_id = record.event.event_id.clone();
        let ingest_result = self.reserve_and_schedule(event_id.clone(), inserted).await;
        
        // Fetch updated record to get the queued status after reserve_and_schedule
        if let Ok(updated_record) = self.store.get(&event_id).await {
            record = updated_record;
        }
        
        (record, ingest_result)
    }

    // Try to reserve and enqueue if needed. If this fails, it's not a critical error, just means the event won't be processed until next retry/sweep.
    pub async fn reserve_and_schedule(&self, event_id: String, inserted: bool) -> IngestResult {
        match self.store.reserve_enqueue_if_needed(&event_id, Utc::now()).await {
            ReserveResult::Noop => {
                IngestResult::Ok(inserted) // Not an error, just means it's already being processed or doesn't need processing
            }
            ReserveResult::Enqueue => {
                // If this fails, it means the handlers are not consuming from the queue, which is a problem, 
                // but we have already marked the record as queued, so we can just return an error and let it be retried later.
                match self.tx.try_send(event_id.clone()) {
                    Ok(_) => {
                        self.telemetry.queue_channel_depth.inc();
                        IngestResult::Ok(inserted)
                    },
                    Err(TrySendError::Closed(e)) => {
                        tracing::error!(event_id = %event_id, "Failed to enqueue event");
                        self.shutdown_tx.send(true).ok(); // Signal shutdown to prevent further processing, since the queue is not working
                        IngestResult::QueueError(format!("Failed to enqueue: {}", e))
                    },
                    Err(TrySendError::Full(_)) => {
                        tracing::warn!(event_id = %event_id, "Queue is full, failed to enqueue event");
                        IngestResult::QueueFull
                    },
                }
            }
        }
    }
}