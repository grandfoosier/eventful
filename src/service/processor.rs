use prometheus::IntGauge;
use rand::prelude::*;
use std::sync::Arc;
use tokio::{
    sync::{mpsc, Semaphore, watch::Receiver}, 
    task::JoinSet, 
    time::{Duration, timeout}
};

use crate::{Telemetry, service::Handler, store::MemoryStore};

struct InflightGuard(IntGauge);

impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.0.dec();
    }
}

pub async fn dispatch_loop(
    store: MemoryStore,
    mut rx: mpsc::Receiver<String>,
    pool_size: usize,
    shutdown_timeout_ms: u64,
    mut shutdown_rx: Receiver<bool>,
    telemetry: Telemetry,
) {
    let semaphore = Arc::new(Semaphore::new(pool_size));
    let mut join_set = JoinSet::new();
    'dispatch: loop { tokio::select! {
        _ = shutdown_rx.changed() => {
            if *shutdown_rx.borrow() {
                println!("Shutdown signal received, stopping dispatch loop");
                break 'dispatch;
            }
        },
        maybe_id = rx.recv() => { match maybe_id {
            Some(id) => {
                telemetry.queue_channel_depth.dec();
                let mut shutdown_rx_clone = shutdown_rx.clone();
                tokio::select! {
                    _ = shutdown_rx_clone.changed() => {
                        if *shutdown_rx_clone.borrow() {
                            println!("Shutdown signal received, stopping dispatch loop");
                            break 'dispatch;
                        }
                    },
                    permit = semaphore.clone().acquire_owned() => { match permit {
                        Ok(permit) => {
                            let store = store.clone();
                            let telemetry = telemetry.clone();
                            telemetry.processing_inflight.inc();
                            let _guard = InflightGuard(telemetry.processing_inflight.clone());
                            let millis;
                            {   let mut rng = rand::rng();
                                millis = rng.random_range(15..45); }
                            let processing_time = Duration::from_millis(millis);
                            join_set.spawn(async move {
                                let _permit = permit; // hold the permit for the duration of this task to limit concurrency
                                let mut handler = Handler::new(store, shutdown_rx_clone, telemetry);
                                handler.run(id, processing_time).await;
                            });
                        },
                        Err(e) => {
                            eprintln!("Semaphore closed, shutting down processor pool: {}", e);
                            break 'dispatch;
                        }
                    }}
                }
            },
            None => {
                println!("All senders dropped, shutting down dispatch loop");
                break 'dispatch;
            }
        }}
    }}
    shutdown(shutdown_timeout_ms, &mut join_set).await;
}

async fn shutdown(shutdown_timeout_ms: u64, join_set: &mut JoinSet<()>) {
    let (mut panics, mut cancels) = (0, 0);
    match timeout(
        Duration::from_millis(shutdown_timeout_ms), 
        join_all(join_set, &mut panics, &mut cancels)
    ).await {
        Ok(_) => println!("All worker tasks completed gracefully"),
        Err(_) => { 
            let remaining = join_set.len();
            if remaining == 0 { println!("Shutdown timeout reached, but no worker tasks remained"); }
            else {
                eprintln!("Shutdown timeout reached, aborting {} remaining worker tasks", remaining);
                join_set.abort_all();
                match timeout(
                    Duration::from_millis(100),
                    join_all(join_set, &mut panics, &mut cancels)
                ).await {
                    Ok(_) => println!("All worker tasks aborted"),
                    Err(_) => {
                        let still_remaining = join_set.len();
                        if still_remaining == 0 { println!("All worker tasks aborted"); }
                        else { eprintln!("Some worker tasks did not abort in time, forcing shutdown with {} tasks remaining", still_remaining); }
                    }
                }
            }
        }
    }
    if panics > 0 { eprintln!("{} worker tasks panicked during shutdown", panics); }
    if cancels > 0 { eprintln!("{} worker tasks were cancelled during shutdown", cancels); }
}

async fn join_all(join_set: &mut JoinSet<()>, panics: &mut usize, cancels: &mut usize) {
    while let Some(opt) = join_set.join_next().await {
        if let Err(join_err) = opt { 
            if join_err.is_panic() { *panics += 1; }
            else if join_err.is_cancelled() { *cancels += 1; } 
            else { eprintln!("Worker task ended with error: {}", join_err); }
        }
    }  
}