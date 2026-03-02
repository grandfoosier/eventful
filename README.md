# Eventful

An in-memory, asynchronous **event processing service** built with **Tokio**, **Axum**, and **Prometheus-based observability**.

Eventful is a portfolio project designed to demonstrate production-grade async Rust patterns while remaining self-contained and easy to reason about.

---

## 🚀 Features

- Idempotent event ingestion
- Bounded async work queue
- Controlled worker concurrency
- Retry with exponential backoff
- Stale queue detection + sweeping
- Graceful shutdown with timeout
- Structured logging (`tracing`)
- Prometheus metrics
- Clear internal state machine

---

## 🏗 Architecture Diagram

```
                      ┌────────────────────┐
                      │      HTTP API      │
                      │  (Axum Handlers)   │
                      └─────────┬──────────┘
                                │
                                ▼
                      ┌────────────────────┐
                      │   IngestService    │
                      │ insert + reserve   │
                      └─────────┬──────────┘
                                │
                                ▼
                      ┌────────────────────┐
                      │    MemoryStore     │
                      │  state machine +   │
                      │ retry scheduling   │
                      └─────────┬──────────┘
                                │
                                ▼
                      ┌────────────────────┐
                      │  Bounded Channel   │
                      │  (mpsc queue)      │
                      └─────────┬──────────┘
                                │
                                ▼
                      ┌────────────────────┐
                      │   Dispatch Loop    │
                      │  Semaphore-limited │
                      │   worker spawning  │
                      └─────────┬──────────┘
                                │
                                ▼
                      ┌────────────────────┐
                      │      Workers       │
                      │ process + retry    │
                      └────────────────────┘

      ┌────────────────────────────────────────────────────┐
      │                     Sweeper                        │
      │  scans for retry-ready + stale queued events       │
      └────────────────────────────────────────────────────┘

      ┌────────────────────────────────────────────────────┐
      │                   Telemetry                        │
      │ counters • gauges • histograms • tracing           │
      └────────────────────────────────────────────────────┘
```

### Core Components

#### HTTP Layer (Axum)

- `POST /events` — ingest an event
- `GET /events/:id` — fetch event state
- `GET /healthz` — service health snapshot
- `GET /metrics` — Prometheus metrics

---

#### MemoryStore

The authoritative in-memory state machine.

Responsibilities:

- Insert-if-absent (idempotency)
- Payload hash validation
- Retry scheduling
- Exponential backoff
- Queue tracking
- Processing state transitions

---

#### IngestService

Bridges HTTP and processing:

- Inserts events
- Reserves for enqueue
- Attempts non-blocking channel send (`try_send`)
- Handles backpressure and shutdown signaling

---

#### Dispatch Loop

- Receives event IDs from channel
- Uses `Semaphore` to limit concurrency
- Spawns worker tasks
- Tracks tasks via `JoinSet`
- Supports graceful shutdown with timeout

---

#### Sweeper

- Runs periodically
- Scans store for:
  - Retry-ready events
  - Stale queued events
- Re-enqueues them safely

Ensures no event is permanently stranded.

---

#### Telemetry (Prometheus)

Observability is a first-class concern in this project.

Metrics include:

### Counters

- `http_requests_total{method,path,status}`
- `events_ingested_total`
- `events_deduped_total`
- `events_processed_total`
- `events_failed_total`

### Gauges

- `queue_channel_depth`
- `backlog_queued`
- `processing_inflight`

### Histogram

- `event_processing_seconds`

Metrics are exposed at:

```
GET /metrics
```

---

## 🔄 Event Lifecycle

### 1️⃣ Ingestion

`POST /events`

- Insert into store (`insert_if_absent`)
- Reject if same ID but different payload (409)
- Reserve for enqueue if runnable
- Attempt channel send (`try_send`)
  - Full → 429
  - Closed → trigger shutdown

---

### 2️⃣ Queued State

```
status = Received or FailedRetry
queued = true
```

Tracked via:

- `queue_channel_depth`
- `backlog_queued`

---

### 3️⃣ Processing

Dispatch loop:

- Receives ID
- Acquires semaphore permit
- Spawns worker task
- Increments `processing_inflight`

Worker:

- Claims event (`try_start_processing`)
- Moves to `Processing`
- Clears `queued`
- Simulates work
- Calls `finish_processing`

---

### 4️⃣ Completion

Possible outcomes:

- `Completed`
- `Failed` (terminal)
- `FailedRetry` (scheduled retry)

Retry backoff:

```
base_backoff_ms * 2^retries_used
(capped at 30 seconds)
```

---

## 📊 State Machine

```
Received
   │
   ▼
Processing
   │
   ├── success → Completed
   │
   └── failure
         ├── retries remaining → FailedRetry
         └── exhausted → Failed
```

`FailedRetry` becomes runnable once `retry_time <= now`.

---

## ⚙️ Concurrency Model

- Bounded channel (`queue_capacity`)
- Bounded worker pool via `Semaphore`
- Tasks tracked with `JoinSet`
- Graceful shutdown with configurable timeout
- Remaining tasks aborted if timeout exceeded

This prevents:

- Unbounded memory growth
- Runaway concurrency
- Zombie tasks on shutdown

---

## 🛑 Backpressure Strategy

Two independent signals:

### Channel Backpressure

If `try_send` returns `Full`:
- Return HTTP 429
- Event remains in store
- Sweeper retries later

### Store Backlog

Tracked via `queued_count`.

Represents events marked `queued == true`, independent of channel occupancy.

---

## 🔁 Stale Queue Recovery

If an event:

- Is marked `queued == true`
- Is not processed within `queue_stale_time_ms`

It becomes eligible for re-enqueue.

The sweeper runs every `sweeper_interval_ms` and ensures eventual processing.

---

## 🧯 Graceful Shutdown

Triggered by:

- Ctrl-C
- Channel closure

Behavior:

1. Stop dispatching new work
2. Wait up to `shutdown_timeout_ms`
3. Abort remaining tasks
4. Report panics and cancellations

Shutdown-triggered failures are marked `FailedRetry` and retried immediately on restart.

---

## ⚙️ Configuration

All runtime behavior controlled via `config.json`:

- `worker_concurrency`
- `queue_capacity`
- `base_backoff_ms`
- `max_retries`
- `queue_stale_time_ms`
- `burst_size`
- `sweeper_interval_ms`
- `shutdown_timeout_ms`

---

## 🧪 Running

```
cargo run
```

Example usage:

```
POST   /events
GET    /events/:id
GET    /healthz
GET    /metrics
```

---

## 🎯 Design Philosophy

This project intentionally mirrors real production patterns:

- Clear state transitions
- Explicit backpressure handling
- Structured observability
- Bounded concurrency
- Deterministic shutdown behavior

It is intentionally over-engineered for an in-memory system to demonstrate architectural discipline.

---

## 🔮 Future Enhancements (Out of Scope for This Project)

- Persistent storage
- Distributed processing
- Dead-letter queue
- OpenTelemetry integration
- Rate limiting middleware
- JSON error responses
- Circuit breakers

---

## 📌 Why This Project Exists

Eventful demonstrates:

- Async Rust concurrency control
- Backpressure modeling
- Retry state machines
- Observability-first design
- Clean separation of concerns

It serves as a portfolio project to showcase production-style service design in Rust.

---

## 🧠 Lessons Learned

Building this service surfaced several important engineering principles:

### 1️⃣ Backpressure Must Be Explicit

Relying solely on channel capacity is insufficient.  
Separating:

- Channel depth (`queue_channel_depth`)
- Durable backlog (`backlog_queued`)

provides clearer operational insight and prevents hidden starvation.

---

### 2️⃣ State Machines Reduce Concurrency Bugs

Making event transitions explicit:

```
Received → Processing → Completed
                    ↘ FailedRetry → Failed
```

eliminated ambiguous intermediate states and simplified retry logic.

---

### 3️⃣ Bounded Concurrency Is Non-Negotiable

Using a `Semaphore` instead of unbounded task spawning ensures:

- Predictable resource usage
- Safer shutdown
- Stable performance under load

---

### 4️⃣ Graceful Shutdown Is Harder Than It Looks

Correct shutdown required:

- Coordinated signaling (`watch::channel`)
- Stopping dispatch
- Waiting with timeout
- Aborting remaining tasks
- Accounting for panics and cancellations

Handling this properly dramatically improves production safety.

---

### 5️⃣ Observability Should Be Designed Early

Adding Prometheus metrics clarified:

- Queue health
- Processing throughput
- Failure rates
- Worker utilization

Knowing the instrumentation design up front aides in designing the architecture and 
reduces technical debt.

---

### 6️⃣ Simplicity Beats Cleverness

Keeping:

- A single authoritative store
- Clear ownership boundaries
- Deterministic state transitions

made concurrency reasoning significantly easier.

Even in an in-memory prototype, architectural discipline matters.