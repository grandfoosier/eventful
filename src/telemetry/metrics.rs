use prometheus::{
    Encoder, Histogram, HistogramOpts, 
    IntCounter, IntCounterVec, IntGauge, 
    Opts, Registry, TextEncoder,
};

#[derive(Clone)]
pub struct Telemetry {
    pub http_requests_total: IntCounterVec,
    pub events_ingested: IntCounter,
    pub events_deduped: IntCounter,
    pub events_processed: IntCounter,
    pub events_failed: IntCounter,
    pub queue_channel_depth: IntGauge,
    pub backlog_queued: IntGauge,
    pub processing_inflight: IntGauge,
    pub processing_hist: Histogram,
    pub registry: Registry,
}

impl Telemetry {
    pub fn new() -> Self {
        let registry = Registry::new();

        let http_requests_total = IntCounterVec::new(
            Opts::new("http_requests_total", "Total HTTP requests"),
            &["method", "path", "status"]
        ).unwrap();
        let events_ingested = IntCounter::with_opts(Opts::new("events_ingested_total", "Total ingested events")).unwrap();
        let events_deduped = IntCounter::with_opts(Opts::new("events_deduped_total", "Total deduped events")).unwrap();
        let events_processed = IntCounter::with_opts(Opts::new("events_processed_total", "Total processed events")).unwrap();
        let events_failed = IntCounter::with_opts(Opts::new("events_failed_total", "Total failed events")).unwrap();
        let queue_channel_depth = IntGauge::with_opts(Opts::new("queue_channel_depth", "Queue channel depth")) .unwrap();
        let backlog_queued = IntGauge::with_opts(Opts::new("backlog_queued", "Backlog queued")) .unwrap();
        let processing_inflight = IntGauge::with_opts(Opts::new("processing_inflight", "Processing inflight")) .unwrap();
        let processing_hist = Histogram::with_opts(HistogramOpts::new("event_processing_seconds", "Event processing duration")) .unwrap();

        registry.register(Box::new(http_requests_total.clone())).ok();
        registry.register(Box::new(events_ingested.clone())).ok();
        registry.register(Box::new(events_deduped.clone())).ok();
        registry.register(Box::new(events_processed.clone())).ok();
        registry.register(Box::new(events_failed.clone())).ok();
        registry.register(Box::new(queue_channel_depth.clone())).ok();
        registry.register(Box::new(backlog_queued.clone())).ok();
        registry.register(Box::new(processing_inflight.clone())).ok();
        registry.register(Box::new(processing_hist.clone())).ok();

        Telemetry {
            http_requests_total,
            events_ingested,
            events_deduped,
            events_processed,
            events_failed,
            queue_channel_depth,
            backlog_queued,
            processing_inflight,
            processing_hist,
            registry
        }
    }

    /// Gather metrics in Prometheus text format.
    pub fn gather(&self) -> String {
        let mut buffer = Vec::new();
        let encoder = TextEncoder::new();
        let mf = self.registry.gather();
        encoder.encode(&mf, &mut buffer).unwrap_or_default();
        String::from_utf8(buffer).unwrap_or_default()
    }
}