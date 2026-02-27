use tokio::sync::watch::Receiver;

use crate::{MemoryStore, domain::EventRecord, store::StartResult};

pub struct Handler {
    store: MemoryStore,
    shutdown_rx: Receiver<bool>, // Since this implementation does not take much time, we will just check the shutdown signal at the beginning of each handling.
}

impl Handler {
    pub fn new(store: MemoryStore, shutdown_rx: Receiver<bool>) -> Self {
        Handler { store, shutdown_rx }
    }

    pub async fn handle_event(&self, attempt: u32, rec: EventRecord) {
        if *self.shutdown_rx.borrow() {
            _ = self.store.finish_processing(&rec.event.event_id, attempt, Err("Shutting down".into())).await;
            return;
        }
        println!("Handling event {:?} with attempt {}", rec.event.event_id, attempt);
        if rec.event.payload.0.get("fail").and_then(|v| v.as_bool()).unwrap_or(false) {
            _ = self.store.finish_processing(&rec.event.event_id, attempt, Err("Simulated failure".into())).await;
        } else {
            _ = self.store.finish_processing(&rec.event.event_id, attempt, Ok(())).await;
        }
    }

    pub async fn run(&mut self, id: String) {
        // Try to claim and handle the event
        match self.store.try_start_processing(&id).await {
            StartResult::Start { attempt, record } => {
                self.handle_event(attempt, record).await;
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