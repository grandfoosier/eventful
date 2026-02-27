use chrono::Utc;
use tokio::{sync::watch::Receiver, time, time::Duration};

use crate::service::ingest::IngestService;
use crate::store::MemoryStore;

pub struct Sweeper {
    store: MemoryStore,
    ingest_service: IngestService,
    sweeper_interval_ms: u64,
}

impl Sweeper {
    pub fn new(
        store: MemoryStore, 
        ingest_service: IngestService, 
        sweeper_interval_ms: u64, 
    ) -> Self { Self {
        store,
        ingest_service,
        sweeper_interval_ms,
    }}

    pub async fn run(&self, shutdown_rx: Receiver<bool>) {
        let mut interval = time::interval(Duration::from_millis(self.sweeper_interval_ms));
        loop {
            interval.tick().await;
            if *shutdown_rx.borrow() { break; }
            self.sweep().await;
        }
    }

    pub async fn sweep(&self) {
        let now = Utc::now();
        let records = self.store.list_enqueue_candidates(now).await;
        for event_id in records {
            // best effort to reserve and enqueue, if this fails, it will be retried in the next sweep
            _ = self.ingest_service.reserve_and_schedule(event_id.clone(), false).await;
        }
    }
}