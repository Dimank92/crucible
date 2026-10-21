# Backend Issues: Standalone 50 Batch [1/50] - [50/50]

This document catalogs the 50 backend architecture, security, performance, and reliability issues identified in the `crucible` backend crate (`backend/`).

---

### [1/50] Blocking Thread Pool Starvation on Sandbox Execution Timeout
• Location: `backend/src/api/handlers/sandbox.rs:31-80`
• Severity: Critical
• Labels: `backend`, `bug`, `api-architecture`, `performance-optimization`
• Description: When a contract execution times out in `tokio::time::timeout`, the underlying `tokio::task::spawn_blocking` task cannot be cancelled. Untrusted Soroban WASM contracts executing infinite loops permanently monopolize blocking worker threads, quickly leading to complete thread pool exhaustion and API server deadlock.
• Remediation: Execute WASM sandboxes inside bounded child processes or pass an atomic cancellation flag / cooperative fuel check to the WASM runtime executor to force-terminate timed-out threads.

---

### [2/50] Missing CPU Instruction Fuel Metering Check in Sandbox Service
• Location: `backend/src/services/sandbox.rs:174-220`
• Severity: High
• Labels: `backend`, `bug`, `performance-optimization`
• Description: `SandboxLimits` specifies `max_cpu_instructions`, but the Soroban execution loop in `SorobanContractExecutor` fails to enforce pre-invocation gas fuel caps or abort early when instructions exceed the allocated threshold before host completion.
• Remediation: Configure Soroban host environment metering with strict fuel bounds (`budget.reset_limits`) prior to invoking contract entrypoints and map fuel exhaustion to `SandboxError::FuelExhausted`.

---

### [3/50] Unchecked Ledger Sequence and Timestamp Overrides in Contract Sandbox
• Location: `backend/src/services/sandbox.rs:65-76`
• Severity: Medium
• Labels: `backend`, `bug`, `data-validation`
• Description: `ContractExecutionRequest` allows clients to supply arbitrary `ledger_sequence` and `ledger_timestamp` without upper or lower bounding against current network consensus time, allowing callers to manipulate time-dependent contract states (timelocks, vesting, auctions).
• Remediation: Enforce validation rules verifying that simulated ledger sequence and timestamp do not diverge beyond configurable bounds from live Stellar network status.

---

### [4/50] Inadequate XDR Argument Depth Validation Leading to Stack Exhaustion
• Location: `backend/src/api/handlers/sandbox.rs:64-76`
• Severity: High
• Labels: `backend`, `bug`, `data-validation`, `auth-security`
• Description: The validation only checks the aggregate byte length of `args_xdr_base64`, but does not inspect recursion depth for deeply nested XDR structures. Deserializing recursive XDR types triggers stack overflow in `soroban_sdk::xdr::FromXdr`.
• Remediation: Use `xdr::Limits::depth` when parsing base64 XDR arguments to reject excessively nested arrays or scval variants.

---

### [5/50] Unauthenticated WebSocket Upgrade Endpoint Leaks Real-Time System Metrics
• Location: `backend/src/api/handlers/ws.rs:45-51`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`, `api-architecture`
• Description: The WebSocket upgrade handler `/api/v1/ws/dashboard` accepts arbitrary connections without validating authorization headers or bearer session tokens, leaking internal system performance metrics and active task error traces to unauthenticated clients.
• Remediation: Add JWT or session token query parameter / header validation during the WebSocket handshake upgrade in `ws_dashboard_handler`.

---

### [6/50] Missing Pong Heartbeat Deadline in WebSocket Connection Stream
• Location: `backend/src/api/handlers/ws.rs:88-112`
• Severity: Medium
• Labels: `backend`, `bug`, `async-workers`, `performance-optimization`
• Description: `ws_dashboard_handler` sends periodic Ping frames every 30 seconds, but does not track client Pong receipt or enforce a heartbeat timeout. Zombie connections remain open indefinitely in kernel socket buffers.
• Remediation: Maintain a `last_pong_received` timestamp; terminate and tear down connections that fail to reply to Pings within a configured timeout window.

---

### [7/50] Unbounded Broadcast Message Buffering on Slow WebSocket Consumers
• Location: `backend/src/api/handlers/ws.rs:77-85`
• Severity: Medium
• Labels: `backend`, `bug`, `performance-optimization`
• Description: Dashboard updates are written directly to `sender.send()` without backpressure flow control. If a client stalls TCP ACKs, outbound messages queue up unbounded in Axum memory channels.
• Remediation: Enforce an outbound channel buffer limit and disconnect slow consumers that drop behind real-time push events.

---

### [8/50] In-Memory Faucet Rate Limiter Bypassed in Multi-Pod Deployments
• Location: `backend/src/api/handlers/stellar.rs:40-68`
• Severity: High
• Labels: `backend`, `bug`, `api-architecture`, `database`
• Description: `FaucetDispenserService` stores IP and destination account rate limits in local `Arc<Mutex<HashMap>>`. In horizontally scaled production deployments, round-robin load balancers distribute requests across instances, completely circumventing rate limits.
• Remediation: Migrate faucet rate limit tracking to a centralized Redis sliding-window counter or token bucket store.

---

### [9/50] Unbounded Memory Leak in Faucet Rate Limiting Table
• Location: `backend/src/api/handlers/stellar.rs:70-80`
• Severity: Medium
• Labels: `backend`, `bug`, `performance-optimization`
• Description: `check_rate_limit` inserts a new vector entry for every novel IP address, but empty entries are never purged after timestamps expire. Under continuous traffic or spoofed IPs, the hashmap expands monotonically.
• Remediation: Implement periodic sweeping or TTL eviction of inactive IP and account keys.

---

### [10/50] Lack of Transaction Memo Validation in Testnet Faucet Dispenser
• Location: `backend/src/api/handlers/stellar.rs:14-30`
• Severity: Low
• Labels: `backend`, `enhancement`, `data-validation`
• Description: `FundAccountRequest` lacks support and validation for Stellar transaction memos (Text, Id, Hash), preventing testnet funding for contracts or exchanges requiring memo-based account routing.
• Remediation: Add optional `memo` field to `FundAccountRequest` with 28-byte text limit validation and pass it into the transaction builder.

---

### [11/50] N+1 Query Pattern in Contract Dashboard Stats Aggregation
• Location: `backend/src/api/handlers/dashboard.rs:180-240`
• Severity: High
• Labels: `backend`, `bug`, `database`, `performance-optimization`
• Description: Fetching stats for multiple contracts issues individual SQL queries in a loop for invocations, error rates, and gas consumption, causing database latency spikes under high dashboard traffic.
• Remediation: Consolidate stats queries into a single batched SQL query using `GROUP BY contract_id` and window functions.

---

### [12/50] Cache Stampede on Dashboard Metrics Recomputation
• Location: `backend/src/api/handlers/dashboard.rs:60-120`
• Severity: Medium
• Labels: `backend`, `bug`, `performance-optimization`, `database`
• Description: When the dashboard metrics Redis cache key expires, all concurrent incoming HTTP requests simultaneously miss the cache and trigger expensive aggregate database queries.
• Remediation: Implement mutex locking or probabilistic early expiration (XFetch algorithm) to ensure only a single worker recomputes expired metrics.

---

### [13/50] Role Elevation Operations Lack Immutable Audit Logging
• Location: `backend/src/api/handlers/admin.rs:40-95`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`
• Description: Admin endpoints granting elevated privileges or modifying user roles execute state transitions without inserting immutable audit event records into the `audit_logs` table.
• Remediation: Wrap user role modification operations in a database transaction that synchronously inserts an audit log entry with actor ID, target user, and previous/new role.

---

### [14/50] Incomplete Session Invalidation on User Suspension or Role Revocation
• Location: `backend/src/api/handlers/admin.rs:100-145`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`
• Description: Suspending a compromised user updates `users.is_active = false` in PostgreSQL, but does not invalidate active JWT tokens or revoke active Redis session keys, allowing suspended accounts to continue calling authorized endpoints until JWT expiration.
• Remediation: Broadcast session revocation events across Redis and blacklist active `jti` tokens associated with suspended accounts.

---

### [15/50] Unauthenticated Alert Ingestion Endpoint Vulnerable to Flood Denial of Service
• Location: `backend/src/api/handlers/alerts.rs:10-50`
• Severity: Medium
• Labels: `backend`, `bug`, `api-architecture`, `auth-security`
• Description: The `/api/v1/alerts/ingest` endpoint lacks API key authentication or HMAC signature verification, allowing unauthorized external actors to flood the system with fake alerts.
• Remediation: Require webhook HMAC signatures or dedicated service tokens via middleware on the alert ingestion route.

---

### [16/50] Missing Deduplication Window for Repetitive Alert Ingestion
• Location: `backend/src/api/handlers/alerts.rs:55-90`
• Severity: Low
• Labels: `backend`, `enhancement`, `async-workers`
• Description: Identical alert payloads ingested within seconds are dispatched repeatedly to downstream notification targets without deduplication or sliding-window suppression.
• Remediation: Compute a SHA-256 fingerprint of `(alert_type, source, message)` and enforce a 5-minute suppression window in Redis.

---

### [17/50] Bytecode Verification Ignores Invalid Export Signatures on Compile
• Location: `backend/src/api/handlers/contracts.rs:90-140`
• Severity: Medium
• Labels: `backend`, `bug`, `data-validation`
• Description: `compile_contract` verifies that WASM magic bytes are present, but does not inspect whether required Soroban host export symbols (e.g. `__constructor`, contract functions) conform to canonical ABI definitions.
• Remediation: Add AST inspection of compiled WASM binaries using `wasmparser` to ensure exported functions adhere to Soroban specification.

---

### [18/50] Unbounded Dependency Graph Recursion in Contract Analysis
• Location: `backend/src/api/handlers/contracts.rs:98-120`
• Severity: Medium
• Labels: `backend`, `bug`, `performance-optimization`
• Description: `analyze_dependencies` recursively traverses nested crate dependency manifests without enforcing a recursion depth or cycle detection limit, leading to stack overflow on cyclic or malicious manifests.
• Remediation: Implement cycle detection and enforce a hard limit on dependency depth (maximum 16 layers).

---

### [19/50] Path Traversal Vulnerability in LCOV Coverage Report Ingestion
• Location: `backend/src/api/handlers/coverage.rs:25-75`
• Severity: Critical
• Labels: `backend`, `bug`, `auth-security`, `data-validation`
• Description: The code coverage report parser extracts source file paths from `SF:` lines in uploaded LCOV files without sanitizing `../` directory traversal sequences, potentially mapping file paths outside the project workspace.
• Remediation: Canonicalize and sanitize parsed source file paths against the repository root using safe path prefix validation.

---

### [20/50] Missing Composite Index on Contract Test Coverage Historical Records
• Location: `backend/src/api/handlers/coverage.rs:80-130`
• Severity: Medium
• Labels: `backend`, `bug`, `database`, `performance-optimization`
• Description: Querying test coverage progression filters by `contract_id` and sorts by `created_at DESC`, performing sequential table scans on large coverage tables.
• Remediation: Add a composite index on `(contract_id, created_at DESC)` in a database migration.

---

### [21/50] Deployment Readiness Probe Does Not Verify Database Write Capability
• Location: `backend/src/api/handlers/deploy_health.rs:30-85`
• Severity: Medium
• Labels: `backend`, `bug`, `devops-deployment`, `monitoring-observability`
• Description: The Kubernetes readiness probe performs a simple `SELECT 1` query which succeeds even if the database is in read-only standby or transaction log disk-full state, routing traffic to unviable pods.
• Remediation: Execute a lightweight transaction check or verify write permissions in readiness health probes.

---

### [22/50] Deployment Health Probe Ignores Worker Queue Backlog and Lag
• Location: `backend/src/api/handlers/deploy_health.rs:90-140`
• Severity: Low
• Labels: `backend`, `enhancement`, `monitoring-observability`
• Description: Health metrics report pod status as healthy even when background ingestion workers are lagging thousands of ledgers behind network tip.
• Remediation: Incorporate ingestion queue lag and worker heartbeat freshness into `/api/v1/health/deploy` response payloads.

---

### [23/50] Gas Optimizer Engine Ignores Contract Storage Footprint Expansion Costs
• Location: `backend/src/api/handlers/gas_optimizer.rs:35-85`
• Severity: Medium
• Labels: `backend`, `bug`, `api-architecture`
• Description: The gas optimization suggestions calculate CPU and memory instructions, but do not estimate Soroban ledger read/write storage byte footprints, underestimating deployment and invocation fees.
• Remediation: Include persistent and temporary storage footprint estimates and rent balance projections in `GasOptimizationReport`.

---

### [24/50] Missing Query Complexity and Depth Limits on GraphQL Endpoint
• Location: `backend/src/api/handlers/graphql.rs:60-110`
• Severity: High
• Labels: `backend`, `bug`, `api-architecture`, `auth-security`
• Description: The GraphQL execution handler accepts deeply nested recursive queries without calculating query complexity score or enforcing depth limits, exposing backend nodes to CPU exhaustion DoS.
• Remediation: Configure query depth limits (max depth 6) and query complexity caps on the GraphQL schema execution layer.

---

### [25/50] JWT Authentication Middleware Omits Issuer and Audience Verification
• Location: `backend/src/api/middleware/auth.rs:45-95`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`
• Description: `jsonwebtoken::decode` validates signatures and expiration times but omits explicit checks for `iss` (issuer) and `aud` (audience), allowing valid JWT tokens minted for staging or other environments to be accepted.
• Remediation: Configure `Validation::new()` with strict `set_issuer` and `set_audience` claims matching production environment configuration.

---

### [26/50] Malformed Authorization Header Handling in Auth Middleware
• Location: `backend/src/api/middleware/auth.rs:110-150`
• Severity: Low
• Labels: `backend`, `bug`, `error-handling`
• Description: Authorization headers with non-ASCII characters or missing `Bearer ` prefix produce opaque 500 Internal Server Errors instead of structured 401 Unauthorized Problem Details.
• Remediation: Gracefully extract bearer tokens and return RFC 7807 compliant 401 errors when Authorization headers are malformed.

---

### [27/50] Tenant Isolation Bypass in Bitmask Permission Checks
• Location: `backend/src/api/middleware/permissions.rs:30-75`
• Severity: Critical
• Labels: `backend`, `bug`, `auth-security`
• Description: The permission evaluation middleware checks whether an authenticated user possesses the required permission bitmask, but does not cross-reference the target organization/tenant ID against the resource URI, allowing cross-tenant data access.
• Remediation: Enforce tenant ownership validation alongside permission bitmask checks in `check_permission`.

---

### [28/50] Client IP Extraction Spoofing Behind Reverse Proxies
• Location: `backend/src/api/middleware/rate_limit.rs:40-80`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`, `api-architecture`
• Description: Rate limiting extracts client IPs from the first element of `X-Forwarded-For` without validating proxy trust. Untrusted clients can spoof arbitrary IP headers to evade rate limits.
• Remediation: Configure trusted proxy CIDR ranges and extract the client IP from the rightmost untrusted hop in `X-Forwarded-For`.

---

### [29/50] Non-Atomic Idempotency Key Release and Response Caching
• Location: `backend/src/api/middleware/idempotency.rs:60-110`
• Severity: Medium
• Labels: `backend`, `bug`, `database`, `api-architecture`
• Description: Storing the completed HTTP response for an idempotency key occurs after releasing the execution lock, creating a window where identical concurrent requests fail with lock conflicts instead of receiving the cached response.
• Remediation: Atomically update the idempotency record from `processing` to `completed` with response payload in a single Redis transaction or Lua script.

---

### [30/50] Race Condition in Concurrent Idempotency Key Acquisition
• Location: `backend/src/api/middleware/idempotency.rs:30-58`
• Severity: Medium
• Labels: `backend`, `bug`, `database`
• Description: Idempotency key checking performs a read followed by a separate write instead of an atomic `SET NX` command. Two simultaneous identical requests both read empty keys and execute concurrently.
• Remediation: Use `SET resource token NX PX <ttl>` to ensure mutually exclusive key reservation.

---

### [31/50] Sensitive Credentials Logged in HTTP Access Logs
• Location: `backend/src/api/middleware/logging.rs:45-90`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`, `monitoring-observability`
• Description: `logging_middleware` captures query parameters and request URLs verbatim without redacting sensitive tokens (`token=`, `secret=`, `api_key=`), leaking credentials into centralized log collectors.
• Remediation: Apply regex/URI query parameter redaction to sanitize query strings before logging requests.

---

### [32/50] Content-Type Header Validation Bypass via Header Parameters
• Location: `backend/src/api/middleware/content_type.rs:15-40`
• Severity: Low
• Labels: `backend`, `bug`, `api-architecture`
• Description: `require_json_content_type` performs exact string matching against `application/json`, rejecting valid requests containing parameters like `application/json; charset=utf-8` while accepting uppercase variants.
• Remediation: Parse `Content-Type` headers using `mime::Mime` and compare only the primary type and subtype case-insensitively.

---

### [33/50] Unbounded Concurrent Requests in Circuit Breaker Half-Open State
• Location: `backend/src/services/circuit_breaker.rs:61-76`
• Severity: High
• Labels: `backend`, `bug`, `async-workers`, `performance-optimization`
• Description: When transitioning from `Open` to `HalfOpen`, `is_request_allowed()` returns `true` for all concurrent requests rather than restricting throughput to a single canary probe, causing immediate thundering herd on recovering external services.
• Remediation: Restrict `HalfOpen` state to allow exactly one in-flight probe request, rejecting all other concurrent requests until the probe succeeds or fails.

---

### [34/50] Premature Circuit Breaker Failure Counter Reset
• Location: `backend/src/services/circuit_breaker.rs:78-93`
• Severity: Low
• Labels: `backend`, `bug`, `error-handling`
• Description: In `record_success`, consecutive failures are reset to zero on any single success in `Closed` state, allowing fluttering flaky services to never breach the failure threshold.
• Remediation: Implement a sliding time window for failure counting rather than immediate reset on a single isolated success.

---

### [35/50] Distributed Lock Heartbeat Loss Does Not Cancel Running Tasks
• Location: `backend/src/services/distributed_lock.rs:245-268`
• Severity: Critical
• Labels: `backend`, `bug`, `async-workers`, `database`
• Description: When the background heartbeat task in `LockGuard` loses Redis quorum, it logs a warning and exits, but does not trigger a cancellation token or notify the holding task. The caller continues executing under the false assumption of mutual exclusion.
• Remediation: Expose a `tokio_util::sync::CancellationToken` on `LockGuard` that is automatically triggered when heartbeat renewal fails or loses quorum.

---

### [36/50] Clock Drift Underestimation in Redlock Quorum Acquisition
• Location: `backend/src/services/distributed_lock.rs:188-195`
• Severity: Medium
• Labels: `backend`, `bug`, `async-workers`
• Description: The Redlock validity calculation computes drift as `ttl * 0.01 + 2ms`, which is insufficient for cross-region or high-latency Redis clusters where network latency variance exceeds 2ms.
• Remediation: Make clock drift adjustment factor configurable in `RedlockConfig` and include standard round-trip network variance.

---

### [37/50] Silent Invalidation Loss During Redis Pub/Sub Partitions
• Location: `backend/src/services/cache_invalidation.rs:50-100`
• Severity: Medium
• Labels: `backend`, `bug`, `database`, `performance-optimization`
• Description: Local in-memory caches rely on Redis Pub/Sub for cross-pod invalidation. During temporary Redis disconnections, Pub/Sub messages are dropped without buffer or acknowledgment, leaving local caches permanently inconsistent.
• Remediation: Track cache generation sequences or fallback to Redis Streams with consumer group acknowledgments for critical cache invalidation events.

---

### [38/50] CPU Spikes from Wildcard Key Eviction in Cache Invalidation
• Location: `backend/src/services/cache_invalidation.rs:120-165`
• Severity: Medium
• Labels: `backend`, `bug`, `performance-optimization`
• Description: Invalidation calls `KEYS pattern:*` to find keys matching wildcards, blocking the single-threaded Redis event loop and causing severe latency spikes on large keyspaces.
• Remediation: Replace `KEYS` commands with non-blocking `SCAN` iteration or organize cache keys into Redis Sets for O(1) group invalidation.

---

### [39/50] Blue-Green Contract Rollout Lacks Transaction Rollback on Migration Error
• Location: `backend/src/services/contract_deployment.rs:75-130`
• Severity: High
• Labels: `backend`, `bug`, `database`, `error-handling`
• Description: When deploying contract upgrades across stages, failures during intermediate state migration leave the deployment registry in an inconsistent intermediate state without automatic rollback.
• Remediation: Execute multi-step deployment operations inside a PostgreSQL transactional savepoint and execute rollback handlers upon step failure.

---

### [40/50] Soroban WASM Upgrade Verification Ignores Deprecated Host Functions
• Location: `backend/src/services/contract_upgrade.rs:45-95`
• Severity: Medium
• Labels: `backend`, `bug`, `data-validation`
• Description: `verify_upgrade_compatibility` verifies bytecode structure but does not scan import tables for deprecated or removed Soroban host function interfaces across protocol versions.
• Remediation: Cross-reference WASM import sections against active Stellar Core protocol host function deprecation tables before approving upgrades.

---

### [41/50] Semantic Version Ordering Parser Fails on Build Metadata
• Location: `backend/src/services/contract_versioning.rs:60-110`
• Severity: Low
• Labels: `backend`, `bug`, `data-validation`
• Description: Contract semantic version comparison treats SemVer build metadata (`+build.123`) as precedence criteria, violating the SemVer 2.0.0 specification where build metadata must be ignored in precedence.
• Remediation: Use `semver::Version` parsing and strip build metadata during version precedence evaluations.

---

### [42/50] Event Indexer Skips Reorganization Reconciliation on Block Hash Mismatch
• Location: `backend/src/services/event_indexer.rs:80-135`
• Severity: High
• Labels: `backend`, `bug`, `database`, `async-workers`
• Description: When the event indexer detects that a previous block hash does not match the parent hash of a newly received ledger, it continues processing without rolling back orphaned ledger events.
• Remediation: Detect blockchain re-orgs, locate the common ancestor ledger, and purge invalidated event records in an atomic database transaction.

---

### [43/50] Unbounded Memory Buffering During Historical Catch-Up Sync
• Location: `backend/src/services/event_indexer.rs:140-190`
• Severity: Medium
• Labels: `backend`, `bug`, `performance-optimization`
• Description: Catch-up synchronization of historical contract events accumulates all parsed events in a single in-memory vector before batch insertion, causing OOM panics when syncing large ledger spans.
• Remediation: Stream historical events through a bounded channel and flush database inserts in fixed-size chunks (e.g., 500 events per chunk).

---

### [44/50] Feature Flag Service Lacks Safe Fallback When Redis Drops
• Location: `backend/src/services/feature_flags.rs:40-85`
• Severity: Medium
• Labels: `backend`, `bug`, `error-handling`
• Description: Calling `is_enabled` attempts to query Redis directly. If Redis is unreachable, the call returns an error or panics instead of falling back to default static flag values.
• Remediation: Cache active feature flags locally in memory with atomic reference swapping, falling back to local defaults during Redis outages.

---

### [45/50] Jitter Calculation in Webhook Dispatch Allows Zero-Millisecond Delay
• Location: `backend/src/workers/webhook.rs:103-118`
• Severity: Low
• Labels: `backend`, `bug`, `async-workers`
• Description: `calculate_retry_delay` computes `mixed % max_delay_ms`, which can evaluate to 0ms. Retrying immediately without delay defeats the purpose of exponential backoff during server overload.
• Remediation: Enforce a minimum backoff floor (e.g. `base_delay / 2`) added to the jittered duration.

---

### [46/50] Missing DNS Rebinding Protection Against SSRF in Webhook Delivery
• Location: `backend/src/workers/webhook.rs:145-160`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`
• Description: While webhook URLs are validated before dispatch, DNS resolution is separated from HTTP client connection establishment. An attacker can use DNS rebinding (low TTL) to pass validation and subsequently connect to internal cloud metadata endpoints (169.254.169.254).
• Remediation: Pin the resolved public IP address and configure `reqwest` to connect directly to the pinned IP with the `Host` header set explicitly.

---

### [47/50] Poison Pill Messages Re-Enter Infinite Replay Loop Without Quarantine
• Location: `backend/src/workers/dlq.rs:30-80`
• Severity: Medium
• Labels: `backend`, `bug`, `async-workers`, `error-handling`
• Description: Replaying dead-letter jobs re-inserts failed payloads into the primary worker queue without recording replay counts. Corrupted jobs that persistently crash workers loop indefinitely through the DLQ.
• Remediation: Track `replay_count` in DLQ metadata and move jobs exceeding maximum replay attempts into a quarantined permanent failure archive.

---

### [48/50] Distributed Scheduler Drops Scheduled Jobs During Leader Failover
• Location: `backend/src/workers/scheduler.rs:60-110`
• Severity: Medium
• Labels: `backend`, `bug`, `async-workers`
• Description: When a leader scheduler pod crashes, the newly elected leader calculates the next cron execution from `now()` rather than inspecting the last recorded execution timestamp, skipping jobs scheduled during the failover window.
• Remediation: Check `last_run_at` in the database upon assuming leadership and trigger missed catch-up executions if within a tolerance window.

---

### [49/50] Non-Transactional Batch Ingestion in Soroban Event Ingestion Worker
• Location: `backend/src/workers/ingestion.rs:120-180`
• Severity: High
• Labels: `backend`, `bug`, `database`, `async-workers`
• Description: Ledger event ingestion writes contract events, transaction logs, and the ledger sequence cursor in separate SQL statements without an enclosing transaction. A worker crash midway leaves the cursor behind or causes duplicate events.
• Remediation: Wrap batch event writes and cursor progression updates in a single atomic SQLx database transaction.

---

### [50/50] Dynamic Config Reload Does Not Validate Connection Pool Caps
• Location: `backend/src/config/reload.rs:80-140`
• Severity: Low
• Labels: `backend`, `bug`, `database`, `data-validation`
• Description: Reloading configuration via SIGHUP updates database connection pool limits in memory, but fails to check whether the new `max_connections` exceeds Postgres server limits or drops below active in-flight query count.
• Remediation: Validate that new pool limits are compatible with current server capacity and gracefully adjust pool bounds without terminating active connections.
