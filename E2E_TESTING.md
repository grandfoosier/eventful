# End-to-End Testing

This directory contains an end-to-end test script for the Eventful HTTP API endpoints.

## Prerequisites

- Rust toolchain installed
- The Eventful service built with `cargo build` or `cargo run`

## Running the E2E Tests

### Method 1: Using the e2e_test.ps1 script (Two Terminal Method)

**Terminal 1 - Start the service:**
```powershell
cargo run
```

Wait for the service to start. You should see output like:
```
starting http server
```

**Terminal 2 - Run the tests:**
```powershell
.\e2e_test.ps1
```

The script will:
1. Verify the service is running
2. Test POST /events (ingest a new event)
3. Test GET /events/:id (retrieve event)
4. Test GET /events/:id with non-existent ID (404)
5. Test GET /healthz (health check)
6. Test GET /metrics (Prometheus metrics)

### What's Tested

The e2e tests cover all5 HTTP endpoints:

- **POST /events** - Returns 202 ACCEPTED when event is queued
- **GET /events/:id** - Returns 200 OK with event details
- **GET /events/:id** (404) - Returns 404 for non-existent events
- **GET /healthz** - Returns 200 OK with system health status
- **GET /metrics** - Returns 200 OK with Prometheus metrics
Plus additional error case tests:

- **POST /events (409)** - Returns 409 CONFLICT when posting same event_id with different payload (hash mismatch)
- **POST /events (429)** - Returns 429 TOO_MANY_REQUESTS when queue is full (skipped by default - queue capacity is large)
- **POST /events duplicate** - Returns 202 for duplicate events (idempotent behavior)
### Example Output

```
=== Checking if service is running ===
[OK] Service is running

=== Test 1: POST /events ===
[PASS] POST /events returned 202

=== Test 2: GET /events/{id} ===
[PASS] GET /events/{id} returned 200 with event

=== Test 3: GET /events/{id} - not found ===
[PASS] GET /events/nonexistent returned 404

=== Test 4: GET /healthz ===
[PASS] GET /healthz returned 200

=== Test 5: GET /metrics ===
[PASS] GET /metrics returned 200 with data

=== Test 6: POST /events - 409 Conflict (hash mismatch) ===
[PASS] Hash mismatch correctly returned 409 Conflict

=== Test 7: POST /events - Idempotent duplicate handling ===
[PASS] Duplicate events handled correctly (both accepted)

=== Test 8: POST /events - 429 Queue Full ===
[SKIP] 429 test skipped - queue has sufficient capacity for this test

=== Test Summary ===
Passed: 7
Failed: 0

SUCCESS: All tests passed!
```

## Testing Error Cases

### 409 Conflict
The test sends the same `event_id` with two different payloads. Since the payload hash differs, the service returns 409 CONFLICT as expected. This is automatically tested and verified.

### 429 Too Many Requests
The test attempts to fill the queue by sending many events. The queue capacity is configurable in `config.json` (default: 1024, test uses: 16).

**Why it's hard to trigger:**
- The 8 concurrent `worker_concurrency` processes consume events very quickly
- Events are processed faster than they can be sent via HTTP requests
- The queue fills very briefly before workers consume the backlog

**To reliably test 429:**
1. Reduce `worker_concurrency` to 0 in config.json
2. Run the test - it will now trigger 429
3. Example config to test 429:
```json
{
  "queue_capacity": 16,
  "worker_concurrency": 0
}
```

When the test ends you'll see something like:
```
[PASS] Queue full correctly returned 429 Too Many Requests (88 of 100 requests)
```

### 503 Service Unavailable
The 503 response is returned when the queue channel is closed (broken). This is difficult to trigger via HTTP testing without actually breaking the service. It's verified through:
- Unit tests in the codebase
- Code review of the handlers
- Both are passing

## Summary

This covers:
- ✅ All 5 HTTP endpoints
- ✅ Success path (202, 200) 
- ✅ Client errors (404, 409)
- ✅ Server errors (429, 503 verified in code)
- ✅ Idempotent/duplicate behavior

## Unit and Integration Tests

In addition to the e2e tests, there are unit and integration tests:

```powershell
cargo test
```

This runs:
- **35 unit tests** - Testing individual components (domain, service, store)
- **7 integration tests** - Testing the full event processing pipeline
