#!/usr/bin/env bash
# ==============================================================================
# Script: publish_backend_50_issues.sh
# Purpose: Automatically publish 50 backend issues to GitHub via `gh issue create`
# Target Repository: benelabs/crucible
# Series: [1/50] - [50/50]
# ==============================================================================

set -euo pipefail

REPO="benelabs/crucible"

echo "=========================================================="
echo " Publishing 50 Backend Issues to ${REPO}"
echo " Active GitHub user: $(gh api user --jq .login || echo 'Unknown')"
echo "=========================================================="

create_issue() {
  local index="$1"
  local title="$2"
  local labels="$3"
  local location="$4"
  local severity="$5"
  local description="$6"
  local remediation="$7"

  local full_title="[${index}/50] ${title}"
  local body="• Location: ${location}
• Severity: ${severity}
• Description: ${description}
• Remediation: ${remediation}"

  echo "==> Creating Issue ${index}/50: ${full_title} ..."

  gh issue create \
    --repo "${REPO}" \
    --title "${full_title}" \
    --body "${body}" \
    --label "${labels}"

  # Sleep briefly to avoid secondary GitHub API rate limiting
  sleep 1.5
}

# ------------------------------------------------------------------------------
# 1-7: API Architecture & Routing
# ------------------------------------------------------------------------------

create_issue "1" \
  "Blocking Thread Pool Starvation on Sandbox Execution Timeout" \
  "backend,bug,Stellar Wave,api-architecture,performance-optimization" \
  "backend/src/api/handlers/sandbox.rs:31-80" \
  "Critical" \
  "When a contract execution times out in tokio::time::timeout, the underlying tokio::task::spawn_blocking task cannot be cancelled. Untrusted Soroban WASM contracts executing infinite loops permanently monopolize blocking worker threads, quickly leading to complete thread pool exhaustion and API server deadlock." \
  "Execute WASM sandboxes inside bounded child processes or pass an atomic cancellation flag / cooperative fuel check to the WASM runtime executor to force-terminate timed-out threads."

create_issue "2" \
  "Missing CPU Instruction Fuel Metering Check in Sandbox Service" \
  "backend,bug,Stellar Wave,performance-optimization" \
  "backend/src/services/sandbox.rs:174-220" \
  "High" \
  "SandboxLimits specifies max_cpu_instructions, but the Soroban execution loop in SorobanContractExecutor fails to enforce pre-invocation gas fuel caps or abort early when instructions exceed the allocated threshold before host completion." \
  "Configure Soroban host environment metering with strict fuel bounds (budget.reset_limits) prior to invoking contract entrypoints and map fuel exhaustion to SandboxError::FuelExhausted."

create_issue "3" \
  "Unchecked Ledger Sequence and Timestamp Overrides in Contract Sandbox" \
  "backend,bug,Stellar Wave,data-validation" \
  "backend/src/services/sandbox.rs:65-76" \
  "Medium" \
  "ContractExecutionRequest allows clients to supply arbitrary ledger_sequence and ledger_timestamp without upper or lower bounding against current network consensus time, allowing callers to manipulate time-dependent contract states (timelocks, vesting, auctions)." \
  "Enforce validation rules verifying that simulated ledger sequence and timestamp do not diverge beyond configurable bounds from live Stellar network status."

create_issue "4" \
  "Inadequate XDR Argument Depth Validation Leading to Stack Exhaustion" \
  "backend,bug,Stellar Wave,data-validation,auth-security" \
  "backend/src/api/handlers/sandbox.rs:64-76" \
  "High" \
  "The validation only checks the aggregate byte length of args_xdr_base64, but does not inspect recursion depth for deeply nested XDR structures. Deserializing recursive XDR types triggers stack overflow in soroban_sdk::xdr::FromXdr." \
  "Use xdr::Limits::depth when parsing base64 XDR arguments to reject excessively nested arrays or scval variants."

create_issue "5" \
  "Unauthenticated WebSocket Upgrade Endpoint Leaks Real-Time System Metrics" \
  "backend,bug,Stellar Wave,auth-security,api-architecture" \
  "backend/src/api/handlers/ws.rs:45-51" \
  "High" \
  "The WebSocket upgrade handler /api/v1/ws/dashboard accepts arbitrary connections without validating authorization headers or bearer session tokens, leaking internal system performance metrics and active task error traces to unauthenticated clients." \
  "Add JWT or session token query parameter / header validation during the WebSocket handshake upgrade in ws_dashboard_handler."

create_issue "6" \
  "Missing Pong Heartbeat Deadline in WebSocket Connection Stream" \
  "backend,bug,Stellar Wave,async-workers,performance-optimization" \
  "backend/src/api/handlers/ws.rs:88-112" \
  "Medium" \
  "ws_dashboard_handler sends periodic Ping frames every 30 seconds, but does not track client Pong receipt or enforce a heartbeat timeout. Zombie connections remain open indefinitely in kernel socket buffers." \
  "Maintain a last_pong_received timestamp; terminate and tear down connections that fail to reply to Pings within a configured timeout window."

create_issue "7" \
  "Unbounded Broadcast Message Buffering on Slow WebSocket Consumers" \
  "backend,bug,Stellar Wave,performance-optimization" \
  "backend/src/api/handlers/ws.rs:77-85" \
  "Medium" \
  "Dashboard updates are written directly to sender.send() without backpressure flow control. If a client stalls TCP ACKs, outbound messages queue up unbounded in Axum memory channels." \
  "Enforce an outbound channel buffer limit and disconnect slow consumers that drop behind real-time push events."

# ------------------------------------------------------------------------------
# 8-14: Stellar RPC, Faucet & Admin Authorization
# ------------------------------------------------------------------------------

create_issue "8" \
  "In-Memory Faucet Rate Limiter Bypassed in Multi-Pod Deployments" \
  "backend,bug,Stellar Wave,api-architecture,database" \
  "backend/src/api/handlers/stellar.rs:40-68" \
  "High" \
  "FaucetDispenserService stores IP and destination account rate limits in local Arc<Mutex<HashMap>>. In horizontally scaled production deployments, round-robin load balancers distribute requests across instances, completely circumventing rate limits." \
  "Migrate faucet rate limit tracking to a centralized Redis sliding-window counter or token bucket store."

create_issue "9" \
  "Unbounded Memory Leak in Faucet Rate Limiting Table" \
  "backend,bug,Stellar Wave,performance-optimization" \
  "backend/src/api/handlers/stellar.rs:70-80" \
  "Medium" \
  "check_rate_limit inserts a new vector entry for every novel IP address, but empty entries are never purged after timestamps expire. Under continuous traffic or spoofed IPs, the hashmap expands monotonically." \
  "Implement periodic sweeping or TTL eviction of inactive IP and account keys."

create_issue "10" \
  "Lack of Transaction Memo Validation in Testnet Faucet Dispenser" \
  "backend,enhancement,Stellar Wave,data-validation" \
  "backend/src/api/handlers/stellar.rs:14-30" \
  "Low" \
  "FundAccountRequest lacks support and validation for Stellar transaction memos (Text, Id, Hash), preventing testnet funding for contracts or exchanges requiring memo-based account routing." \
  "Add optional memo field to FundAccountRequest with 28-byte text limit validation and pass it into the transaction builder."

create_issue "11" \
  "N+1 Query Pattern in Contract Dashboard Stats Aggregation" \
  "backend,bug,Stellar Wave,database,performance-optimization" \
  "backend/src/api/handlers/dashboard.rs:180-240" \
  "High" \
  "Fetching stats for multiple contracts issues individual SQL queries in a loop for invocations, error rates, and gas consumption, causing database latency spikes under high dashboard traffic." \
  "Consolidate stats queries into a single batched SQL query using GROUP BY contract_id and window functions."

create_issue "12" \
  "Cache Stampede on Dashboard Metrics Recomputation" \
  "backend,bug,Stellar Wave,performance-optimization,database" \
  "backend/src/api/handlers/dashboard.rs:60-120" \
  "Medium" \
  "When the dashboard metrics Redis cache key expires, all concurrent incoming HTTP requests simultaneously miss the cache and trigger expensive aggregate database queries." \
  "Implement mutex locking or probabilistic early expiration (XFetch algorithm) to ensure only a single worker recomputes expired metrics."

create_issue "13" \
  "Role Elevation Operations Lack Immutable Audit Logging" \
  "backend,bug,Stellar Wave,auth-security" \
  "backend/src/api/handlers/admin.rs:40-95" \
  "High" \
  "Admin endpoints granting elevated privileges or modifying user roles execute state transitions without inserting immutable audit event records into the audit_logs table." \
  "Wrap user role modification operations in a database transaction that synchronously inserts an audit log entry with actor ID, target user, and previous/new role."

create_issue "14" \
  "Incomplete Session Invalidation on User Suspension or Role Revocation" \
  "backend,bug,Stellar Wave,auth-security" \
  "backend/src/api/handlers/admin.rs:100-145" \
  "High" \
  "Suspending a compromised user updates users.is_active = false in PostgreSQL, but does not invalidate active JWT tokens or revoke active Redis session keys, allowing suspended accounts to continue calling authorized endpoints until JWT expiration." \
  "Broadcast session revocation events across Redis and blacklist active jti tokens associated with suspended accounts."

# ------------------------------------------------------------------------------
# 15-21: Alerts, Compilation & Static Analysis
# ------------------------------------------------------------------------------

create_issue "15" \
  "Unauthenticated Alert Ingestion Endpoint Vulnerable to Flood Denial of Service" \
  "backend,bug,Stellar Wave,api-architecture,auth-security" \
  "backend/src/api/handlers/alerts.rs:10-50" \
  "Medium" \
  "The /api/v1/alerts/ingest endpoint lacks API key authentication or HMAC signature verification, allowing unauthorized external actors to flood the system with fake alerts." \
  "Require webhook HMAC signatures or dedicated service tokens via middleware on the alert ingestion route."

create_issue "16" \
  "Missing Deduplication Window for Repetitive Alert Ingestion" \
  "backend,enhancement,Stellar Wave,async-workers" \
  "backend/src/api/handlers/alerts.rs:55-90" \
  "Low" \
  "Identical alert payloads ingested within seconds are dispatched repeatedly to downstream notification targets without deduplication or sliding-window suppression." \
  "Compute a SHA-256 fingerprint of (alert_type, source, message) and enforce a 5-minute suppression window in Redis."

create_issue "17" \
  "Bytecode Verification Ignores Invalid Export Signatures on Compile" \
  "backend,bug,Stellar Wave,data-validation" \
  "backend/src/api/handlers/contracts.rs:90-140" \
  "Medium" \
  "compile_contract verifies that WASM magic bytes are present, but does not inspect whether required Soroban host export symbols (e.g. __constructor, contract functions) conform to canonical ABI definitions." \
  "Add AST inspection of compiled WASM binaries using wasmparser to ensure exported functions adhere to Soroban specification."

create_issue "18" \
  "Unbounded Dependency Graph Recursion in Contract Analysis" \
  "backend,bug,Stellar Wave,performance-optimization" \
  "backend/src/api/handlers/contracts.rs:98-120" \
  "Medium" \
  "analyze_dependencies recursively traverses nested crate dependency manifests without enforcing a recursion depth or cycle detection limit, leading to stack overflow on cyclic or malicious manifests." \
  "Implement cycle detection and enforce a hard limit on dependency depth (maximum 16 layers)."

create_issue "19" \
  "Path Traversal Vulnerability in LCOV Coverage Report Ingestion" \
  "backend,bug,Stellar Wave,auth-security,data-validation" \
  "backend/src/api/handlers/coverage.rs:25-75" \
  "Critical" \
  "The code coverage report parser extracts source file paths from SF: lines in uploaded LCOV files without sanitizing ../ directory traversal sequences, potentially mapping file paths outside the project workspace." \
  "Canonicalize and sanitize parsed source file paths against the repository root using safe path prefix validation."

create_issue "20" \
  "Missing Composite Index on Contract Test Coverage Historical Records" \
  "backend,bug,Stellar Wave,database,performance-optimization" \
  "backend/src/api/handlers/coverage.rs:80-130" \
  "Medium" \
  "Querying test coverage progression filters by contract_id and sorts by created_at DESC, performing sequential table scans on large coverage tables." \
  "Add a composite index on (contract_id, created_at DESC) in a database migration."

create_issue "21" \
  "Deployment Readiness Probe Does Not Verify Database Write Capability" \
  "backend,bug,Stellar Wave,devops-deployment,monitoring-observability" \
  "backend/src/api/handlers/deploy_health.rs:30-85" \
  "Medium" \
  "The Kubernetes readiness probe performs a simple SELECT 1 query which succeeds even if the database is in read-only standby or transaction log disk-full state, routing traffic to unviable pods." \
  "Execute a lightweight transaction check or verify write permissions in readiness health probes."

# ------------------------------------------------------------------------------
# 22-28: Probes, Gas Optimizer & Middleware Security
# ------------------------------------------------------------------------------

create_issue "22" \
  "Deployment Health Probe Ignores Worker Queue Backlog and Lag" \
  "backend,enhancement,Stellar Wave,monitoring-observability" \
  "backend/src/api/handlers/deploy_health.rs:90-140" \
  "Low" \
  "Health metrics report pod status as healthy even when background ingestion workers are lagging thousands of ledgers behind network tip." \
  "Incorporate ingestion queue lag and worker heartbeat freshness into /api/v1/health/deploy response payloads."

create_issue "23" \
  "Gas Optimizer Engine Ignores Contract Storage Footprint Expansion Costs" \
  "backend,bug,Stellar Wave,api-architecture" \
  "backend/src/api/handlers/gas_optimizer.rs:35-85" \
  "Medium" \
  "The gas optimization suggestions calculate CPU and memory instructions, but do not estimate Soroban ledger read/write storage byte footprints, underestimating deployment and invocation fees." \
  "Include persistent and temporary storage footprint estimates and rent balance projections in GasOptimizationReport."

create_issue "24" \
  "Missing Query Complexity and Depth Limits on GraphQL Endpoint" \
  "backend,bug,Stellar Wave,api-architecture,auth-security" \
  "backend/src/api/handlers/graphql.rs:60-110" \
  "High" \
  "The GraphQL execution handler accepts deeply nested recursive queries without calculating query complexity score or enforcing depth limits, exposing backend nodes to CPU exhaustion DoS." \
  "Configure query depth limits (max depth 6) and query complexity caps on the GraphQL schema execution layer."

create_issue "25" \
  "JWT Authentication Middleware Omits Issuer and Audience Verification" \
  "backend,bug,Stellar Wave,auth-security" \
  "backend/src/api/middleware/auth.rs:45-95" \
  "High" \
  "jsonwebtoken::decode validates signatures and expiration times but omits explicit checks for iss (issuer) and aud (audience), allowing valid JWT tokens minted for staging or other environments to be accepted." \
  "Configure Validation::new() with strict set_issuer and set_audience claims matching production environment configuration."

create_issue "26" \
  "Malformed Authorization Header Handling in Auth Middleware" \
  "backend,bug,Stellar Wave,error-handling" \
  "backend/src/api/middleware/auth.rs:110-150" \
  "Low" \
  "Authorization headers with non-ASCII characters or missing Bearer prefix produce opaque 500 Internal Server Errors instead of structured 401 Unauthorized Problem Details." \
  "Gracefully extract bearer tokens and return RFC 7807 compliant 401 errors when Authorization headers are malformed."

create_issue "27" \
  "Tenant Isolation Bypass in Bitmask Permission Checks" \
  "backend,bug,Stellar Wave,auth-security" \
  "backend/src/api/middleware/permissions.rs:30-75" \
  "Critical" \
  "The permission evaluation middleware checks whether an authenticated user possesses the required permission bitmask, but does not cross-reference the target organization/tenant ID against the resource URI, allowing cross-tenant data access." \
  "Enforce tenant ownership validation alongside permission bitmask checks in check_permission."

create_issue "28" \
  "Client IP Extraction Spoofing Behind Reverse Proxies" \
  "backend,bug,Stellar Wave,auth-security,api-architecture" \
  "backend/src/api/middleware/rate_limit.rs:40-80" \
  "High" \
  "Rate limiting extracts client IPs from the first element of X-Forwarded-For without validating proxy trust. Untrusted clients can spoof arbitrary IP headers to evade rate limits." \
  "Configure trusted proxy CIDR ranges and extract the client IP from the rightmost untrusted hop in X-Forwarded-For."

# ------------------------------------------------------------------------------
# 29-35: Idempotency, Logging & Circuit Breakers
# ------------------------------------------------------------------------------

create_issue "29" \
  "Non-Atomic Idempotency Key Release and Response Caching" \
  "backend,bug,Stellar Wave,database,api-architecture" \
  "backend/src/api/middleware/idempotency.rs:60-110" \
  "Medium" \
  "Storing the completed HTTP response for an idempotency key occurs after releasing the execution lock, creating a window where identical concurrent requests fail with lock conflicts instead of receiving the cached response." \
  "Atomically update the idempotency record from processing to completed with response payload in a single Redis transaction or Lua script."

create_issue "30" \
  "Race Condition in Concurrent Idempotency Key Acquisition" \
  "backend,bug,Stellar Wave,database" \
  "backend/src/api/middleware/idempotency.rs:30-58" \
  "Medium" \
  "Idempotency key checking performs a read followed by a separate write instead of an atomic SET NX command. Two simultaneous identical requests both read empty keys and execute concurrently." \
  "Use SET resource token NX PX <ttl> to ensure mutually exclusive key reservation."

create_issue "31" \
  "Sensitive Credentials Logged in HTTP Access Logs" \
  "backend,bug,Stellar Wave,auth-security,monitoring-observability" \
  "backend/src/api/middleware/logging.rs:45-90" \
  "High" \
  "logging_middleware captures query parameters and request URLs verbatim without redacting sensitive tokens (token=, secret=, api_key=), leaking credentials into centralized log collectors." \
  "Apply regex/URI query parameter redaction to sanitize query strings before logging requests."

create_issue "32" \
  "Content-Type Header Validation Bypass via Header Parameters" \
  "backend,bug,Stellar Wave,api-architecture" \
  "backend/src/api/middleware/content_type.rs:15-40" \
  "Low" \
  "require_json_content_type performs exact string matching against application/json, rejecting valid requests containing parameters like application/json; charset=utf-8 while accepting uppercase variants." \
  "Parse Content-Type headers using mime::Mime and compare only the primary type and subtype case-insensitively."

create_issue "33" \
  "Unbounded Concurrent Requests in Circuit Breaker Half-Open State" \
  "backend,bug,Stellar Wave,async-workers,performance-optimization" \
  "backend/src/services/circuit_breaker.rs:61-76" \
  "High" \
  "When transitioning from Open to HalfOpen, is_request_allowed() returns true for all concurrent requests rather than restricting throughput to a single canary probe, causing immediate thundering herd on recovering external services." \
  "Restrict HalfOpen state to allow exactly one in-flight probe request, rejecting all other concurrent requests until the probe succeeds or fails."

create_issue "34" \
  "Premature Circuit Breaker Failure Counter Reset" \
  "backend,bug,Stellar Wave,error-handling" \
  "backend/src/services/circuit_breaker.rs:78-93" \
  "Low" \
  "In record_success, consecutive failures are reset to zero on any single success in Closed state, allowing fluttering flaky services to never breach the failure threshold." \
  "Implement a sliding time window for failure counting rather than immediate reset on a single isolated success."

create_issue "35" \
  "Distributed Lock Heartbeat Loss Does Not Cancel Running Tasks" \
  "backend,bug,Stellar Wave,async-workers,database" \
  "backend/src/services/distributed_lock.rs:245-268" \
  "Critical" \
  "When the background heartbeat task in LockGuard loses Redis quorum, it logs a warning and exits, but does not trigger a cancellation token or notify the holding task. The caller continues executing under the false assumption of mutual exclusion." \
  "Expose a tokio_util::sync::CancellationToken on LockGuard that is automatically triggered when heartbeat renewal fails or loses quorum."

# ------------------------------------------------------------------------------
# 36-41: Redlock, Invalidation & Upgrades
# ------------------------------------------------------------------------------

create_issue "36" \
  "Clock Drift Underestimation in Redlock Quorum Acquisition" \
  "backend,bug,Stellar Wave,async-workers" \
  "backend/src/services/distributed_lock.rs:188-195" \
  "Medium" \
  "The Redlock validity calculation computes drift as ttl * 0.01 + 2ms, which is insufficient for cross-region or high-latency Redis clusters where network latency variance exceeds 2ms." \
  "Make clock drift adjustment factor configurable in RedlockConfig and include standard round-trip network variance."

create_issue "37" \
  "Silent Invalidation Loss During Redis Pub/Sub Partitions" \
  "backend,bug,Stellar Wave,database,performance-optimization" \
  "backend/src/services/cache_invalidation.rs:50-100" \
  "Medium" \
  "Local in-memory caches rely on Redis Pub/Sub for cross-pod invalidation. During temporary Redis disconnections, Pub/Sub messages are dropped without buffer or acknowledgment, leaving local caches permanently inconsistent." \
  "Track cache generation sequences or fallback to Redis Streams with consumer group acknowledgments for critical cache invalidation events."

create_issue "38" \
  "CPU Spikes from Wildcard Key Eviction in Cache Invalidation" \
  "backend,bug,Stellar Wave,performance-optimization" \
  "backend/src/services/cache_invalidation.rs:120-165" \
  "Medium" \
  "Invalidation calls KEYS pattern:* to find keys matching wildcards, blocking the single-threaded Redis event loop and causing severe latency spikes on large keyspaces." \
  "Replace KEYS commands with non-blocking SCAN iteration or organize cache keys into Redis Sets for O(1) group invalidation."

create_issue "39" \
  "Blue-Green Contract Rollout Lacks Transaction Rollback on Migration Error" \
  "backend,bug,Stellar Wave,database,error-handling" \
  "backend/src/services/contract_deployment.rs:75-130" \
  "High" \
  "When deploying contract upgrades across stages, failures during intermediate state migration leave the deployment registry in an inconsistent intermediate state without automatic rollback." \
  "Execute multi-step deployment operations inside a PostgreSQL transactional savepoint and execute rollback handlers upon step failure."

create_issue "40" \
  "Soroban WASM Upgrade Verification Ignores Deprecated Host Functions" \
  "backend,bug,Stellar Wave,data-validation" \
  "backend/src/services/contract_upgrade.rs:45-95" \
  "Medium" \
  "verify_upgrade_compatibility verifies bytecode structure but does not scan import tables for deprecated or removed Soroban host function interfaces across protocol versions." \
  "Cross-reference WASM import sections against active Stellar Core protocol host function deprecation tables before approving upgrades."

create_issue "41" \
  "Semantic Version Ordering Parser Fails on Build Metadata" \
  "backend,bug,Stellar Wave,data-validation" \
  "backend/src/services/contract_versioning.rs:60-110" \
  "Low" \
  "Contract semantic version comparison treats SemVer build metadata (+build.123) as precedence criteria, violating the SemVer 2.0.0 specification where build metadata must be ignored in precedence." \
  "Use semver::Version parsing and strip build metadata during version precedence evaluations."

# ------------------------------------------------------------------------------
# 42-50: Event Indexer, Webhooks, DLQ & Runtime Configuration
# ------------------------------------------------------------------------------

create_issue "42" \
  "Event Indexer Skips Reorganization Reconciliation on Block Hash Mismatch" \
  "backend,bug,Stellar Wave,database,async-workers" \
  "backend/src/services/event_indexer.rs:80-135" \
  "High" \
  "When the event indexer detects that a previous block hash does not match the parent hash of a newly received ledger, it continues processing without rolling back orphaned ledger events." \
  "Detect blockchain re-orgs, locate the common ancestor ledger, and purge invalidated event records in an atomic database transaction."

create_issue "43" \
  "Unbounded Memory Buffering During Historical Catch-Up Sync" \
  "backend,bug,Stellar Wave,performance-optimization" \
  "backend/src/services/event_indexer.rs:140-190" \
  "Medium" \
  "Catch-up synchronization of historical contract events accumulates all parsed events in a single in-memory vector before batch insertion, causing OOM panics when syncing large ledger spans." \
  "Stream historical events through a bounded channel and flush database inserts in fixed-size chunks (e.g., 500 events per chunk)."

create_issue "44" \
  "Feature Flag Service Lacks Safe Fallback When Redis Drops" \
  "backend,bug,Stellar Wave,error-handling" \
  "backend/src/services/feature_flags.rs:40-85" \
  "Medium" \
  "Calling is_enabled attempts to query Redis directly. If Redis is unreachable, the call returns an error or panics instead of falling back to default static flag values." \
  "Cache active feature flags locally in memory with atomic reference swapping, falling back to local defaults during Redis outages."

create_issue "45" \
  "Jitter Calculation in Webhook Dispatch Allows Zero-Millisecond Delay" \
  "backend,bug,Stellar Wave,async-workers" \
  "backend/src/workers/webhook.rs:103-118" \
  "Low" \
  "calculate_retry_delay computes mixed % max_delay_ms, which can evaluate to 0ms. Retrying immediately without delay defeats the purpose of exponential backoff during server overload." \
  "Enforce a minimum backoff floor (e.g. base_delay / 2) added to the jittered duration."

create_issue "46" \
  "Missing DNS Rebinding Protection Against SSRF in Webhook Delivery" \
  "backend,bug,Stellar Wave,auth-security" \
  "backend/src/workers/webhook.rs:145-160" \
  "High" \
  "While webhook URLs are validated before dispatch, DNS resolution is separated from HTTP client connection establishment. An attacker can use DNS rebinding (low TTL) to pass validation and subsequently connect to internal cloud metadata endpoints (169.254.169.254)." \
  "Pin the resolved public IP address and configure reqwest to connect directly to the pinned IP with the Host header set explicitly."

create_issue "47" \
  "Poison Pill Messages Re-Enter Infinite Replay Loop Without Quarantine" \
  "backend,bug,Stellar Wave,async-workers,error-handling" \
  "backend/src/workers/dlq.rs:30-80" \
  "Medium" \
  "Replaying dead-letter jobs re-inserts failed payloads into the primary worker queue without recording replay counts. Corrupted jobs that persistently crash workers loop indefinitely through the DLQ." \
  "Track replay_count in DLQ metadata and move jobs exceeding maximum replay attempts into a quarantined permanent failure archive."

create_issue "48" \
  "Distributed Scheduler Drops Scheduled Jobs During Leader Failover" \
  "backend,bug,Stellar Wave,async-workers" \
  "backend/src/workers/scheduler.rs:60-110" \
  "Medium" \
  "When a leader scheduler pod crashes, the newly elected leader calculates the next cron execution from now() rather than inspecting the last recorded execution timestamp, skipping jobs scheduled during the failover window." \
  "Check last_run_at in the database upon assuming leadership and trigger missed catch-up executions if within a tolerance window."

create_issue "49" \
  "Non-Transactional Batch Ingestion in Soroban Event Ingestion Worker" \
  "backend,bug,Stellar Wave,database,async-workers" \
  "backend/src/workers/ingestion.rs:120-180" \
  "High" \
  "Ledger event ingestion writes contract events, transaction logs, and the ledger sequence cursor in separate SQL statements without an enclosing transaction. A worker crash midway leaves the cursor behind or causes duplicate events." \
  "Wrap batch event writes and cursor progression updates in a single atomic SQLx database transaction."

create_issue "50" \
  "Dynamic Config Reload Does Not Validate Connection Pool Caps" \
  "backend,bug,Stellar Wave,database,data-validation" \
  "backend/src/config/reload.rs:80-140" \
  "Low" \
  "Reloading configuration via SIGHUP updates database connection pool limits in memory, but fails to check whether the new max_connections exceeds Postgres server limits or drops below active in-flight query count." \
  "Validate that new pool limits are compatible with current server capacity and gracefully adjust pool bounds without terminating active connections."

echo "=========================================================="
echo " Finished publishing all 50 issues successfully!"
echo "=========================================================="
