use std::time::Duration;
use tokio::{sync::watch::Receiver, time::{Instant, sleep}};

use crate::{MemoryStore, Telemetry, domain::EventRecord, store::StartResult};

pub struct Handler {
    store: MemoryStore,
    shutdown_rx: Receiver<bool>,
    telemetry: Telemetry,
}

impl Handler {
    pub fn new(store: MemoryStore, shutdown_rx: Receiver<bool>, telemetry: Telemetry) -> Self {
        Handler { store, shutdown_rx, telemetry }
    }

    pub async fn handle_event(&self, attempt: u32, rec: EventRecord, processing_time: Duration) {
        if *self.shutdown_rx.borrow() {
            _ = self.store.finish_processing(&rec.event.event_id, attempt, Err("Shutting down".into())).await;
            return;
        }
        println!("Handling event {:?} with attempt {}", rec.event.event_id, attempt);
        let start = Instant::now();
        sleep(processing_time).await;
        if rec.event.payload.0.get("fail").and_then(|v| v.as_bool()).unwrap_or(false) {
            _ = self.store.finish_processing(&rec.event.event_id, attempt, Err("Simulated failure".into())).await;
            self.telemetry.events_failed.inc();
        } else {
            _ = self.store.finish_processing(&rec.event.event_id, attempt, Ok(())).await;
            self.telemetry.events_processed.inc();
        }
        let elapsed = start.elapsed();
        self.telemetry.processing_hist.observe(elapsed.as_secs_f64());
    }

    pub async fn run(&mut self, id: String, processing_time: Duration) {
        // Try to claim and handle the event
        match self.store.try_start_processing(&id).await {
            StartResult::Start { attempt, record } => {
                self.handle_event(attempt, record, processing_time).await;
            },
            StartResult::NotFound => {
                // Invariant violation: we should not have received an event ID that does not exist in the store, 
                // because we only enqueue after reserving, which requires the record to exist. Log and skip if this happens.
                eprintln!("Event {} not found when trying to start processing", id);
            },
            // These cases are not necessarily errors, it just means we do not need to process this event
            StartResult::SkipAlreadyCompleted => {
                println!("Event {} already completed, skipping", id);
            },
            StartResult::SkipNotRunnable => {
                println!("Event {} not runnable (maybe being processed by another worker), skipping", id);
            },
        }
    }
}
