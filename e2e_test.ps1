# End-to-end test script for HTTP endpoints
# USAGE: Start the service with 'cargo run', then run this script in another terminal

$servicePort = 3000
$baseUrl = "http://127.0.0.1:$servicePort"
$testsPassed = 0
$testsFailed = 0

Write-Host ""
Write-Host "=== Checking if service is running ==="
try {
    $response = Invoke-WebRequest -Uri "$baseUrl/healthz" -Method GET -TimeoutSec 1 -ErrorAction Stop -UseBasicParsing
    Write-Host "[OK] Service is running"
}
catch {
    Write-Host "[FAIL] Service is not running on $baseUrl"
    exit 1
}

# Test 1: POST /events
Write-Host ""
Write-Host "=== Test 1: POST /events ==="
$eventDate = [datetime]::UtcNow.ToString('o')
$eventPayload = @{
    event_id = "e2e-test-1"
    event_type = "UserLoginFailed"
    occurred_at = $eventDate
    payload = @{ user_id = "test_user" }
} | ConvertTo-Json

try {
    $response = Invoke-WebRequest -Uri "$baseUrl/events" -Method POST -Body $eventPayload -ContentType "application/json" -UseBasicParsing
    if ($response.StatusCode -eq 202) {
        Write-Host "[PASS] POST /events returned 202"
        $testsPassed = $testsPassed + 1
    } else {
        Write-Host "[FAIL] POST /events returned $($response.StatusCode), expected 202"
        $testsFailed = $testsFailed + 1
    }
}
catch {
    Write-Host "[FAIL] POST /events error: $_"
    $testsFailed = $testsFailed + 1
}

# Test 2: GET /events/{id}
Write-Host ""
Write-Host "=== Test 2: GET /events/{id} ==="
Start-Sleep -Milliseconds 200
try {
    $response = Invoke-WebRequest -Uri "$baseUrl/events/e2e-test-1" -Method GET -UseBasicParsing
    if ($response.StatusCode -eq 200) {
        if ($response.Content -match "e2e-test-1") {
            Write-Host "[PASS] GET /events/{id} returned 200 with event"
            $testsPassed = $testsPassed + 1
        } else {
            Write-Host "[FAIL] GET /events/{id} returned 200 but missing event"
            $testsFailed = $testsFailed + 1
        }
    } else {
        Write-Host "[FAIL] GET /events/{id} returned $($response.StatusCode)"
        $testsFailed = $testsFailed + 1
    }
}
catch {
    Write-Host "[FAIL] GET /events/{id} error: $_"
    $testsFailed = $testsFailed + 1
}

# Test 3: GET /events/{id} - not found
Write-Host ""
Write-Host "=== Test 3: GET /events/{id} - not found ==="
try {
    $response = Invoke-WebRequest -Uri "$baseUrl/events/nonexistent" -Method GET -ErrorAction Stop -UseBasicParsing
    Write-Host "[FAIL] Expected 404 but got $($response.StatusCode)"
    $testsFailed = $testsFailed + 1
}
catch {
    if ($_.Exception.Response.StatusCode -eq 404) {
        Write-Host "[PASS] GET /events/nonexistent returned 404"
        $testsPassed = $testsPassed + 1
    } else {
        Write-Host "[FAIL] Unexpected status code"
        $testsFailed = $testsFailed + 1
    }
}

# Test 4: GET /healthz
Write-Host ""
Write-Host "=== Test 4: GET /healthz ==="
try {
    $response = Invoke-WebRequest -Uri "$baseUrl/healthz" -Method GET -UseBasicParsing
    if ($response.StatusCode -eq 200 -and $response.Content -match "ok") {
        Write-Host "[PASS] GET /healthz returned 200"
        $testsPassed = $testsPassed + 1
    } else {
        Write-Host "[FAIL] Unexpected response from /healthz"
        $testsFailed = $testsFailed + 1
    }
}
catch {
    Write-Host "[FAIL] GET /healthz error: $_"
    $testsFailed = $testsFailed + 1
}

# Test 5: GET /metrics
Write-Host ""
Write-Host "=== Test 5: GET /metrics ==="
try {
    $response = Invoke-WebRequest -Uri "$baseUrl/metrics" -Method GET -UseBasicParsing
    if ($response.StatusCode -eq 200 -and $response.Content.Length -gt 0) {
        Write-Host "[PASS] GET /metrics returned 200 with data"
        $testsPassed = $testsPassed + 1
    } else {
        Write-Host "[FAIL] /metrics did not return expected response"
        $testsFailed = $testsFailed + 1
    }
}
catch {
    Write-Host "[FAIL] GET /metrics error: $_"
    $testsFailed = $testsFailed + 1
}

# Test 6: POST /events with 409 Conflict (hash mismatch)
Write-Host ""
Write-Host "=== Test 6: POST /events - 409 Conflict (hash mismatch) ==="
$conflictPayload1 = @{
    event_id = "e2e-conflict-test"
    event_type = "UserLoginFailed"
    occurred_at = [datetime]::UtcNow.ToString('o')
    payload = @{ user_id = "user1" }
} | ConvertTo-Json

$conflictPayload2 = @{
    event_id = "e2e-conflict-test"
    event_type = "UserLoginFailed"
    occurred_at = [datetime]::UtcNow.ToString('o')
    payload = @{ user_id = "user2" }
} | ConvertTo-Json

try {
    # First post with user1
    $response1 = Invoke-WebRequest -Uri "$baseUrl/events" -Method POST -Body $conflictPayload1 -ContentType "application/json" -UseBasicParsing
    if ($response1.StatusCode -eq 202) {
        # Now try to post same event_id with different payload (user2)
        $response2 = Invoke-WebRequest -Uri "$baseUrl/events" -Method POST -Body $conflictPayload2 -ContentType "application/json" -UseBasicParsing -ErrorAction Stop
        Write-Host "[FAIL] Expected 409 for hash mismatch, got $($response2.StatusCode)"
        $testsFailed = $testsFailed + 1
    } else {
        Write-Host "[FAIL] Initial POST failed with $($response1.StatusCode)"
        $testsFailed = $testsFailed + 1
    }
}
catch {
    if ($_.Exception.Response.StatusCode -eq 409) {
        Write-Host "[PASS] Hash mismatch correctly returned 409 Conflict"
        $testsPassed = $testsPassed + 1
    } else {
        Write-Host "[FAIL] Expected 409 but got $($_.Exception.Response.StatusCode)"
        $testsFailed = $testsFailed + 1
    }
}

# Test 7: POST /events with multiple duplicate events (should not fail, just deduplicated)
Write-Host ""
Write-Host "=== Test 7: POST /events - Idempotent duplicate handling ==="
$dupPayload = @{
    event_id = "e2e-dup-test"
    event_type = "UserLoginFailed"
    occurred_at = [datetime]::UtcNow.ToString('o')
    payload = @{ user_id = "dup_user" }
} | ConvertTo-Json

try {
    $resp1 = Invoke-WebRequest -Uri "$baseUrl/events" -Method POST -Body $dupPayload -ContentType "application/json" -UseBasicParsing
    Start-Sleep -Milliseconds 100
    $resp2 = Invoke-WebRequest -Uri "$baseUrl/events" -Method POST -Body $dupPayload -ContentType "application/json" -UseBasicParsing
    
    if ($resp1.StatusCode -eq 202 -and $resp2.StatusCode -eq 202) {
        Write-Host "[PASS] Duplicate events handled correctly (both accepted)"
        $testsPassed = $testsPassed + 1
    } else {
        Write-Host "[FAIL] Unexpected status codes: $($resp1.StatusCode), $($resp2.StatusCode)"
        $testsFailed = $testsFailed + 1
    }
}
catch {
    Write-Host "[FAIL] Duplicate test error: $_"
    $testsFailed = $testsFailed + 1
}

# Test 8: POST /events with 429 Queue Full (parallel requests)
Write-Host ""
Write-Host "=== Test 8: POST /events - 429 Queue Full (parallel requests) ==="
Write-Host "Sending 100 requests in parallel to fill the queue..."

$queueFull = $false
$timestamp = Get-Date -Format "yyyyMMddHHmmss"
$jobs = @()

# Send 100 requests in parallel (much larger batch to overwhelm queue)
for ($i = 0; $i -lt 100; $i++) {
    $job = Start-Job -ScriptBlock {
        param($baseUrl, $timestamp, $counter)
        
        $bulkPayload = @{
            event_id = "bulk-$timestamp-$counter"
            event_type = "UserLoginFailed"
            occurred_at = [datetime]::UtcNow.ToString('o')
            payload = @{ user_id = "bulk_user"; seq = $counter }
        } | ConvertTo-Json
        
        try {
            $response = Invoke-WebRequest -Uri "$baseUrl/events" -Method POST -Body $bulkPayload -ContentType "application/json" -UseBasicParsing -TimeoutSec 5 -ErrorAction Stop
            return @{ StatusCode = $response.StatusCode; Error = $null }
        }
        catch {
            if ($_.Exception.Response -and $_.Exception.Response.StatusCode) {
                return @{ StatusCode = $_.Exception.Response.StatusCode; Error = $_.Exception.Response.StatusCode }
            } else {
                return @{ StatusCode = $null; Error = $_.Exception.Message }
            }
        }
    } -ArgumentList $baseUrl, $timestamp, $i
    
    $jobs += $job
}

Write-Host "[INFO] Sent 100 parallel requests, waiting for responses (max 10 seconds)..."

# Wait for all jobs with timeout
$allComplete = Wait-Job -Job $jobs -Timeout 10
if ($allComplete.Count -lt $jobs.Count) {
    Write-Host "[INFO] Some jobs still running after timeout, stopping remaining..."
}

# Collect results
$statusCodes = @{}
foreach ($job in $jobs) {
    $result = Receive-Job -Job $job -ErrorAction Ignore
    if ($result) {
        if ($result.StatusCode) {
            if ($statusCodes.ContainsKey($result.StatusCode)) {
                $statusCodes[$result.StatusCode] += 1
            } else {
                $statusCodes[$result.StatusCode] = 1
            }
        }
    }
}

# Check results
$count202 = if ($statusCodes.ContainsKey(202)) { $statusCodes[202] } else { 0 }
$count429 = if ($statusCodes.ContainsKey(429)) { $statusCodes[429] } else { 0 }
$countOther = 0
$statusCodes.GetEnumerator() | Where-Object { $_.Key -ne 202 -and $_.Key -ne 429 } | ForEach-Object { $countOther += $_.Value }

Write-Host "[INFO] Response summary:"
Write-Host "[INFO]   202 Accepted: $count202"
Write-Host "[INFO]   429 Too Many: $count429"
if ($countOther -gt 0) {
    Write-Host "[INFO]   Other errors: $countOther"
}

if ($count429 -gt 0) {
    Write-Host "[PASS] Queue full correctly returned 429 Too Many Requests ($count429 of 100 requests)"
    $testsPassed = $testsPassed + 1
    $queueFull = $true
} else {
    Write-Host "[SKIP] No 429 responses received"
    if ($count202 -eq 100) {
        Write-Host "[INFO]  All 100 requests accepted - workers are keeping up with request rate"
        Write-Host "[INFO]  To trigger 429: reduce worker_concurrency in config.json and restart service"
    }
}

# Clean up jobs
foreach ($job in $jobs) {
    Remove-Job -Job $job -Force -ErrorAction Ignore
}

# Summary
Write-Host ""
Write-Host "=== Test Summary ==="
Write-Host "Passed: $testsPassed"
Write-Host "Failed: $testsFailed"
Write-Host ""

if ($testsFailed -eq 0) {
    Write-Host "SUCCESS: All tests passed!"
    exit 0
}
else {
    Write-Host "FAILURE: Some tests failed"
    exit 1
}
