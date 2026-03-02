use anyhow::Result;
use axum::serve;
use serde::Deserialize;
use std::{fs, sync::Arc};
use tokio::{net::TcpListener, signal, sync::{mpsc, watch}};

use eventful::http::{handlers::HttpState, routes::build_router};
use eventful::service::{dispatch_loop, IngestService, sweeper::Sweeper};
use eventful::store::MemoryStore;
use eventful::telemetry::{logging::init_tracing, Telemetry};

#[derive(Debug, Deserialize)]
#[serde(default)]
struct Config {
    address: String,
    worker_concurrency: usize,
    base_backoff_ms: u64,
    max_retries: u32,
    queue_capacity: usize,
    queue_stale_time_ms: u64,
    burst_size: usize,
    sweeper_interval_ms: u64,
    shutdown_timeout_ms: u64,
    trace_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            address: "127.0.0.1:3000".to_string(),
            worker_concurrency: 8,
            base_backoff_ms: 100,
            max_retries: 5,
            queue_capacity: 1024,
            queue_stale_time_ms: 3000,
            burst_size: 128,
            sweeper_interval_ms: 1000,
            shutdown_timeout_ms: 10000,
            trace_level: "info".into(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let content = fs::read_to_string("config.json")?;
    let config: Config = serde_json::from_str(&content)?;

    init_tracing(config.trace_level, false);
    let telemetry = Telemetry::new();
    let store = MemoryStore::new(
        config.base_backoff_ms, 
        config.max_retries,
        config.queue_stale_time_ms,
        config.burst_size
    );
    let (tx, rx) = mpsc::channel::<String>(config.queue_capacity);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let ingest_service = IngestService::new(store.clone(), tx, shutdown_tx.clone(), telemetry.clone());
    let sweeper = Sweeper::new(
        store.clone(),
        ingest_service.clone(),
        config.sweeper_interval_ms,
    );
    let app_state = HttpState {
        ingest: ingest_service.clone(),
        store: ingest_service.store.clone(),
        telemetry: telemetry.clone(),
    };
    let router = build_router(Arc::new(app_state));
    let listener = TcpListener::bind(&config.address).await?;

    tracing::info!(%config.address, "starting background processing");
    tokio::spawn({
        let store = store.clone();
        let shutdown_rx = shutdown_rx.clone();
        async move { 
            dispatch_loop(
                store, 
                rx, 
                config.worker_concurrency, 
                config.shutdown_timeout_ms, 
                shutdown_rx,
                telemetry.clone(),
            ).await
        }
    });
    tracing::info!(%config.address, "starting queue sweeper");
    tokio::spawn({
        let shutdown_rx = shutdown_rx.clone();
        async move { sweeper.run(shutdown_rx).await; }
    });
    // axum bind and serve
    tracing::info!(%config.address, "starting http server");
    let mut shutdown_rx_server = shutdown_rx.clone();
    serve(listener, router)
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = ctrl_c_signal() => { let _ = shutdown_tx.send(true); },
                _ = async { while !*shutdown_rx_server.borrow() {
                    if shutdown_rx_server.changed().await.is_err() { break; } // sender dropped 
                }} => {}
            }
        })
    .await?;

    Ok(())
}

async fn ctrl_c_signal() {
    signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    // Optional: Add logging or cleanup code here
    tracing::info!("Received Ctrl+C, starting graceful shutdown...");
}
