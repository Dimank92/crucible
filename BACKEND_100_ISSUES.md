# Crucible Backend Issues: Complete 100 Series

This document catalogs the complete suite of 100 backend architecture, security, performance, database, worker, and reliability issues for the `crucible` backend crate (`backend/`).

None of these issues include the `Stellar Wave` tag.

---

## Batch 1: Issues [1/50] – [50/50] (Published #1137 – #1186)

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

---

## Batch 2: Issues [51/100] – [100/100]

### [51/100] Bytecode Vulnerability Scanner Relies on Naive Byte Pattern Matching
• Location: `backend/src/services/vulnerability_scanner.rs:54-88`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`, `data-validation`
• Description: `scan_bytecode` flags reentrancy, unchecked arithmetic, and uninitialized storage by checking if arbitrary raw bytes (such as `0x54`, which is the standard `i32.wrap_i64` WASM opcode) appear anywhere in the binary, causing massive false positives on benign contracts.
• Remediation: Replace raw byte slice matching with proper WebAssembly AST parsing using `wasmparser` to inspect instructions in their semantic context.

---

### [52/100] Source Vulnerability Scanner Inspects Raw Strings Without AST Scoping
• Location: `backend/src/services/vulnerability_scanner.rs:106-125`
• Severity: Medium
• Labels: `backend`, `bug`, `auth-security`, `data-validation`
• Description: `scan_source` checks for substrings like `unsafe`, `unwrap()`, or `expect(` via raw substring search, triggering high-severity alerts on comments, test modules, and string literals.
• Remediation: Use `syn` to parse Rust source code into an abstract syntax tree and evaluate unsafe blocks and unwraps only within active non-test function bodies.

---

### [53/100] Vulnerability Scanner Fails to Report Source Code Line Numbers
• Location: `backend/src/services/vulnerability_scanner.rs:13-15`
• Severity: Low
• Labels: `backend`, `enhancement`, `api-architecture`, `error-handling`
• Description: All discovered vulnerabilities populate `line_number` as `None`, preventing API consumers and developers from locating the vulnerability in their contract source files.
• Remediation: Extract line and column coordinates from `syn` or DWARF debug symbols during scanning and populate `line_number` in `ScanResult`.

---

### [54/100] Contract Benchmark Historical Metrics Lost on Backend Restart
• Location: `backend/src/services/contract_benchmark.rs:11-20`
• Severity: Medium
• Labels: `backend`, `bug`, `database`, `performance-optimization`
• Description: Benchmark execution history is held purely in an in-memory `VecDeque` behind an `RwLock` with a hard limit of 50 entries, losing all historical regression baselines and latency trends across server restarts.
• Remediation: Persist benchmark runs and baseline metrics into PostgreSQL tables with an indexed schema for historical time-series queries.

---

### [55/100] Potential Division by Zero in Benchmark Aggregate Calculations
• Location: `backend/src/services/contract_benchmark.rs:110-140`
• Severity: Low
• Labels: `backend`, `bug`, `performance-optimization`, `error-handling`
• Description: When computing average duration or instruction metrics for benchmark runs with empty sample sets, the service risks producing `NaN` or infinite float values that break JSON serialization.
• Remediation: Guard aggregate computation routines against empty sample collections and return explicit zeroed or optional summaries.

---

### [56/100] Storage Rent Estimation Ignores Dynamic Ledger Configuration
• Location: `backend/src/services/contract_storage_optimizer.rs:25-36`
• Severity: Medium
• Labels: `backend`, `bug`, `api-architecture`, `data-validation`
• Description: The storage rent savings calculation uses hardcoded rent constants rather than fetching active network parameters from the Soroban RPC `getLatestLedger` endpoint.
• Remediation: Query the active network ledger configuration parameters dynamically to calculate accurate storage rent projections.

---

### [57/100] Storage Diff Generator XDR Key Stringification Causes Collision
• Location: `backend/src/services/contract_storage_optimizer.rs:50-59`
• Severity: Medium
• Labels: `backend`, `bug`, `data-validation`, `database`
• Description: Converting complex Soroban storage `DataKey`s to plain string keys via Debug formatting produces truncated or identical strings for distinct enum variants, corrupting storage diff comparisons.
• Remediation: Serialize `DataKey`s to canonical base64-encoded XDR strings to guarantee unique and deterministic storage key identities.

---

### [58/100] Synchronous Contract Invocation Logging Blocks Request Pipeline
• Location: `backend/src/services/contract_call_logger.rs:40-80`
• Severity: High
• Labels: `backend`, `bug`, `performance-optimization`, `async-workers`
• Description: Contract call logger executes direct database writes synchronously during HTTP request handling, causing request latencies to spike under high-frequency contract interaction loads.
• Remediation: Buffer contract invocation logs in a bounded crossbeam/mpsc channel and flush them in batches via an asynchronous background worker.

---

### [59/100] Contract Call Logs Table Lacks Automated Partitioning and TTL Retention
• Location: `backend/src/services/contract_call_logger.rs:85-120`
• Severity: Medium
• Labels: `backend`, `enhancement`, `database`, `devops-deployment`
• Description: The `contract_call_logs` table accumulates millions of invocation records indefinitely without time-based partitioning or automatic pruning policies, risking PostgreSQL disk space exhaustion.
• Remediation: Implement PostgreSQL declarative monthly table partitioning on `created_at` and a scheduled retention cleaner.

---

### [60/100] Contract Metadata Registry Overwrite Lacks Ownership Authorization
• Location: `backend/src/services/contract_registry.rs:30-65`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`, `database`
• Description: Registering or updating contract metadata in the registry does not verify whether the caller holds administrative privileges or original deployer rights for the specified contract ID.
• Remediation: Enforce deployer signature verification or organization-level permission checks prior to modifying contract registry records.

---

### [61/100] Contract Health Monitor Polling Interval Drifts Under Event Loop Pressure
• Location: `backend/src/services/contract_monitor.rs:40-85`
• Severity: Low
• Labels: `backend`, `bug`, `async-workers`, `monitoring-observability`
• Description: The contract health monitor uses sleep-based polling loops that accumulate scheduling drift when asynchronous tasks block the Tokio worker runtime.
• Remediation: Replace manual sleep loops with `tokio::time::interval` configured with `MissedTickBehavior::Skip` for steady cadence monitoring.

---

### [62/100] Contract Test Results Ingestion Lacks Cryptographic Checksum Validation
• Location: `backend/src/services/contract_test_results.rs:25-60`
• Severity: Medium
• Labels: `backend`, `bug`, `data-validation`, `auth-security`
• Description: Ingesting contract test run results accepts client-supplied pass/fail counts and coverage numbers without verifying an HMAC digest or test execution witness signature.
• Remediation: Implement signed test result receipts verifying that test assertions were actually run in a verified Crucible test runner sandbox.

---

### [63/100] Analytics Aggregator Computes Percentiles Using In-Memory Full Sorting
• Location: `backend/src/services/analytics_aggregator.rs:45-90`
• Severity: Medium
• Labels: `backend`, `bug`, `performance-optimization`, `database`
• Description: Percentile latencies (p50, p95, p99) are calculated by loading all raw invocation samples from the database into a heap-allocated vector and sorting in memory, exhausting memory on large query ranges.
• Remediation: Offload percentile calculations to PostgreSQL `percentile_cont` or compute approximate percentiles using t-digest algorithms.

---

### [64/100] Missing Pre-Aggregated Rollups for Historical Contract Activity
• Location: `backend/src/services/analytics_aggregator.rs:95-140`
• Severity: Medium
• Labels: `backend`, `enhancement`, `database`, `performance-optimization`
• Description: Historical dashboard analytics queries re-scan raw event tables for multi-day windows instead of querying daily and hourly pre-aggregated summary tables.
• Remediation: Create hourly and daily rollup tables populated by a background cron job for instant historical analytics responses.

---

### [65/100] Cumulative Business Metrics Counter Vulnerable to Integer Overflow
• Location: `backend/src/services/business_metrics.rs:50-95`
• Severity: Low
• Labels: `backend`, `bug`, `data-validation`, `performance-optimization`
• Description: Cumulative volume and stroop fee aggregators use standard `u64` addition without checked or saturating arithmetic, risking integer overflow on long-running production environments.
• Remediation: Upgrade cumulative metric accumulators to `rust_decimal::Decimal` or `u128` with `saturating_add`.

---

### [66/100] Compliance Sanction Verification Blocks Without Request Timeout
• Location: `backend/src/services/compliance.rs:30-70`
• Severity: High
• Labels: `backend`, `bug`, `api-architecture`, `error-handling`
• Description: Calling the external compliance / OFAC verification service relies on default HTTP client settings without an explicit request timeout, hanging caller threads if the upstream compliance API hangs.
• Remediation: Wrap compliance checks in a strict 3-second timeout with fallback to temporary manual-review queues.

---

### [67/100] Build Request Deduplication TTL Shorter Than Long Compilation Jobs
• Location: `backend/src/services/dedup.rs:35-80`
• Severity: Medium
• Labels: `backend`, `bug`, `async-workers`, `performance-optimization`
• Description: The build deduplication lock has a static 30-second TTL in Redis. Complex contract compilations taking longer than 30 seconds lose their deduplication key, triggering duplicate compilation runs.
• Remediation: Extend deduplication locks dynamically using heartbeat renewals or compute TTL proportional to contract crate dependencies.

---

### [68/100] Documentation Generator Crashes on Complex Unnamed Tuple Struct Returns
• Location: `backend/src/services/doc_generator.rs:40-85`
• Severity: Medium
• Labels: `backend`, `bug`, `error-handling`, `data-validation`
• Description: The OpenAPI / markdown documentation generator panics when parsing Soroban function return signatures containing deeply nested unnamed tuple structures.
• Remediation: Implement robust recursive AST visitors in `doc_generator` that gracefully format tuple structs and complex enum variants.

---

### [69/100] Unsanitized Markdown in Contract Documentation Generator Renders XSS
• Location: `backend/src/services/doc_generator.rs:90-130`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`, `api-architecture`
• Description: Docstrings extracted from uploaded contract source code are rendered directly into OpenAPI description fields without HTML escaping, enabling Stored XSS attacks in Swagger UI.
• Remediation: Pass all extracted docstrings through an HTML sanitization filter (such as `ammonia`) before embedding them into OpenAPI schema documentation.

---

### [70/100] Error Recovery Manager Lacks Exponential Backoff for Persistent Failures
• Location: `backend/src/services/error_recovery.rs:25-70`
• Severity: Medium
• Labels: `backend`, `bug`, `async-workers`, `error-handling`
• Description: Tasks flagged for automatic error recovery are retried immediately on fixed intervals, rapidly consuming system resources when an external dependency is down.
• Remediation: Implement truncated exponential backoff with jitter for all automated task recovery attempts.

---

### [71/100] gRPC Server Lacks Keepalive and Maximum Connection Age Configuration
• Location: `backend/src/services/grpc.rs:30-65`
• Severity: Medium
• Labels: `backend`, `bug`, `api-architecture`, `performance-optimization`
• Description: Tonic gRPC server initialization omits keepalive ping intervals, connection timeouts, and max connection age, leading to stale dead connections persisting through infrastructure load balancers.
• Remediation: Configure `http2_keepalive_interval`, `http2_keepalive_timeout`, and `max_connection_age` on the Tonic Server builder.

---

### [72/100] Unbounded Inbound Message Size on gRPC Contract Streaming Endpoint
• Location: `backend/src/services/grpc.rs:70-100`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`, `api-architecture`
• Description: The gRPC contract deployment stream does not set `max_decoding_message_size`, allowing malicious clients to stream oversized byte buffers and trigger out-of-memory crashes.
• Remediation: Enforce a 10MB `max_decoding_message_size` limit on the gRPC contract service.

---

### [73/100] High Cardinality Route Dimensions in HTTP Metrics Prometheus Collector
• Location: `backend/src/services/http_metrics.rs:20-60`
• Severity: Medium
• Labels: `backend`, `bug`, `monitoring-observability`, `performance-optimization`
• Description: The HTTP metrics middleware includes raw request paths with un-parameterized dynamic UUID path segments in Prometheus metric labels, causing unbounded time-series cardinality explosion.
• Remediation: Normalize URL paths to their route pattern templates (e.g. `/api/v1/contracts/:id`) before recording Prometheus metrics.

---

### [74/100] Log Aggregator Channel Drops Warning Logs Under Burst Traffic
• Location: `backend/src/services/log_aggregator.rs:25-60`
• Severity: Medium
• Labels: `backend`, `bug`, `monitoring-observability`, `async-workers`
• Description: The log aggregation channel uses `try_send` with a small buffer, silently discarding warning and error log entries during traffic surges without recording dropped message counters.
• Remediation: Increase channel capacity and maintain an atomic `dropped_logs_total` metric counter when buffer backpressure occurs.

---

### [75/100] Synchronous Regular Expression Evaluation in Log Alert Engine
• Location: `backend/src/services/log_alerts.rs:40-90`
• Severity: Medium
• Labels: `backend`, `bug`, `performance-optimization`, `async-workers`
• Description: Evaluating log alert trigger conditions recompiles and evaluates regular expressions synchronously across every incoming log record, consuming significant CPU cycles.
• Remediation: Pre-compile regex patterns with `regex::RegexSet` and evaluate log streams asynchronously on dedicated worker threads.

---

### [76/100] Deployment Rollback Engine Does Not Restore Contract Temporary Storage
• Location: `backend/src/services/rollback.rs:35-85`
• Severity: High
• Labels: `backend`, `bug`, `database`, `error-handling`
• Description: The rollback service restores contract WASM bytecode and instance storage, but does not revert temporary or persistent storage entries modified by failed migration scripts.
• Remediation: Include complete snapshotting of contract storage keyspaces prior to upgrades and restore all storage tiers on rollback.

---

### [77/100] Security Scanner Does Not Validate WASM Section Header Offsets
• Location: `backend/src/services/security_scanner.rs:40-85`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`, `data-validation`
• Description: Disassembling WASM binaries for unsafe instructions parses raw byte offsets without validating section lengths, causing panics on truncated or malformed WebAssembly files.
• Remediation: Validate all WebAssembly section boundaries and payload sizes using `wasmparser::Parser` before attempting instruction extraction.

---

### [78/100] System Metrics Collector Re-Initializes Hardware Probes on Every Scrape
• Location: `backend/src/services/sys_metrics.rs:60-110`
• Severity: Medium
• Labels: `backend`, `bug`, `monitoring-observability`, `performance-optimization`
• Description: Every scrape of `/metrics` re-creates a `sysinfo::System` instance and performs full disk and CPU hardware enumeration, adding tens of milliseconds of latency to metrics endpoints.
• Remediation: Maintain a singleton `System` instance that refreshes periodically in the background (e.g. every 5 seconds) rather than synchronously on every HTTP scrape.

---

### [79/100] Coverage Source Map Resolution Fails on Macro-Expanded Functions
• Location: `backend/src/services/test_coverage.rs:50-95`
• Severity: Medium
• Labels: `backend`, `bug`, `testing-infrastructure`, `data-validation`
• Description: Mapping test execution instruction counters back to source lines fails on contracts using procedural derive macros (like `#[contractimpl]`), resulting in misleading 0% line coverage reports.
• Remediation: Incorporate macro expansion tracking and synthetic span mapping into coverage line resolution algorithms.

---

### [80/100] OpenTelemetry OTLP Exporter Lacks Disk Buffer for Network Outages
• Location: `backend/src/services/tracing.rs:45-90`
• Severity: Low
• Labels: `backend`, `bug`, `monitoring-observability`, `error-handling`
• Description: When the OpenTelemetry collector endpoint is unreachable, OTLP span exports drop telemetry silently once in-memory ring buffers fill up.
• Remediation: Implement persistent local spooling or disk-backed fallback buffers for telemetry spans during collector outages.

---

### [81/100] Task Execution Worker Silently Discards Panics Without Diagnostic Trace
• Location: `backend/src/workers/executor.rs:40-85`
• Severity: High
• Labels: `backend`, `bug`, `async-workers`, `error-handling`
• Description: Background task execution handles `tokio::task::JoinError` by marking the job as failed, but does not extract or log the panic payload or backtrace, hindering debugging of worker crashes.
• Remediation: Use `std::panic::catch_unwind` or `join_error.into_panic()` to extract and persist the panic message and backtrace into the failed job record.

---

### [82/100] Missing Per-Task Timeout Enforcement in Background Worker Executor
• Location: `backend/src/workers/executor.rs:90-130`
• Severity: High
• Labels: `backend`, `bug`, `async-workers`, `performance-optimization`
• Description: Background jobs run indefinitely without an upper execution deadline. If a worker gets stuck in an infinite network wait or deadlocked mutex, worker concurrency slots are leaked permanently.
• Remediation: Enforce a configurable per-job timeout (e.g., 5 minutes) via `tokio::time::timeout` on all executor tasks.

---

### [83/100] Cache Pre-Warming Worker Queries Entire Contract Database Without Pagination
• Location: `backend/src/workers/cache_warm.rs:30-70`
• Severity: Medium
• Labels: `backend`, `bug`, `performance-optimization`, `database`
• Description: The cache warming worker executes a `SELECT * FROM contracts` query without a `LIMIT` or cursor, causing query timeouts and huge memory spikes on production databases with thousands of contracts.
• Remediation: Implement cursor-based keyset pagination in the cache pre-warming worker.

---

### [84/100] Job History Automated Pruning Locks Job Runs Table
• Location: `backend/src/workers/job_history.rs:35-75`
• Severity: Medium
• Labels: `backend`, `bug`, `database`, `async-workers`
• Description: Pruning expired job run records uses `DELETE FROM job_runs WHERE finished_at < NOW() - INTERVAL '30 days'` in a single unbounded query, taking exclusive table locks and blocking active job insertions.
• Remediation: Execute deletions in bounded batches (e.g. DELETE in chunks of 1,000 with a sleep) or migrate `job_runs` to partitioned tables.

---

### [85/100] Low Priority Task Queue Starvation Under Continuous High Priority Load
• Location: `backend/src/workers/priority.rs:40-85`
• Severity: Medium
• Labels: `backend`, `bug`, `async-workers`, `performance-optimization`
• Description: The priority task scheduler strictly drains the high-priority queue before inspecting medium or low queues. Sustained high-priority workloads indefinitely starve lower-priority background tasks.
• Remediation: Implement a weighted fair queuing algorithm (e.g. 70% high, 20% medium, 10% low) to guarantee forward progress for lower priority jobs.

---

### [86/100] Job Progress Notifications Skipped on Sub-Second Execution Stages
• Location: `backend/src/workers/progress.rs:30-65`
• Severity: Low
• Labels: `backend`, `enhancement`, `async-workers`, `api-architecture`
• Description: Background task progress updates only broadcast over WebSockets if the progress delta exceeds 10%, causing fast sub-tasks to jump from 0% straight to 100% without client visibility.
• Remediation: Emit explicit phase transition events regardless of numerical percentage progress.

---

### [87/100] Dead-Letter Queue Replay Lacks Jitter Leading to Synchronized Retry Waves
• Location: `backend/src/workers/retry.rs:60-105`
• Severity: Medium
• Labels: `backend`, `bug`, `async-workers`, `performance-optimization`
• Description: Bulk-replaying dead-letter jobs schedules all retries with identical delay timers, overwhelming downstream external APIs with synchronized thundering herd spikes.
• Remediation: Introduce randomized jitter into bulk DLQ retry scheduling.

---

### [88/100] Overly Permissive CORS Configuration in Non-Production Environments
• Location: `backend/src/config/cors.rs:15-35`
• Severity: Medium
• Labels: `backend`, `bug`, `auth-security`, `api-architecture`
• Description: `cors.rs` defaults to `AllowOrigin::any()` and allows credentials when environment is not set to production, exposing developer and staging setups to cross-origin credential theft.
• Remediation: Explicitly reject `AllowOrigin::any()` in combination with `allow_credentials(true)` across all environments.

---

### [89/100] Database Connection Acquire Timeout Too High Causing Request Stalls
• Location: `backend/src/config/database.rs:25-50`
• Severity: Medium
• Labels: `backend`, `bug`, `database`, `performance-optimization`
• Description: Database connection acquisition timeout is set to 30 seconds. When the database pool is exhausted, incoming HTTP requests stall for 30 seconds before failing, causing cascading frontend timeouts.
• Remediation: Reduce connection acquire timeout to 3-5 seconds and return an immediate 503 Service Unavailable when the pool is saturated.

---

### [90/100] Missing TCP Keepalive on PostgreSQL Connection Pool
• Location: `backend/src/config/database.rs:55-80`
• Severity: Medium
• Labels: `backend`, `bug`, `database`, `devops-deployment`
• Description: SQLx `PgPool` configuration does not configure TCP keepalive probes. Intermediate cloud firewalls and NAT gateways silently drop idle connections, causing subsequent queries to hang until TCP timeout.
• Remediation: Configure `PgPoolOptions` with `tcp_keepalive(Some(Duration::from_secs(60)))`.

---

### [91/100] Redis Connection Manager Does Not Reconnect on Initial Startup Failure
• Location: `backend/src/config/redis.rs:20-55`
• Severity: High
• Labels: `backend`, `bug`, `database`, `error-handling`
• Description: If Redis is booting up or temporarily unreachable during backend startup, `redis::Client::get_connection_manager` fails immediately and aborts the entire application without retrying.
• Remediation: Wrap initial Redis connection manager creation in an exponential retry loop with a configurable startup timeout.

---

### [92/100] HTTP/2 Server Listener Lacks Max Concurrent Streams Bound
• Location: `backend/src/config/server.rs:30-65`
• Severity: High
• Labels: `backend`, `bug`, `api-architecture`, `performance-optimization`
• Description: The Axum HTTP listener does not restrict HTTP/2 max concurrent streams, allowing an attacker to open thousands of concurrent HTTP/2 streams over a single TCP connection (HTTP/2 Rapid Reset vulnerability).
• Remediation: Configure `http2_max_concurrent_streams(Some(100))` on the Hyper server builder.

---

### [93/100] Server Graceful Shutdown Does Not Drain Background Worker Jobs
• Location: `backend/src/config/server.rs:70-110`
• Severity: High
• Labels: `backend`, `bug`, `async-workers`, `devops-deployment`
• Description: Receiving a SIGTERM triggers Axum graceful shutdown for HTTP requests, but background workers and scheduled ingestion loops are terminated abruptly, leaving in-flight jobs in a half-committed state.
• Remediation: Broadcast a shutdown cancellation token to all background workers and await worker termination before exiting the process.

---

### [94/100] Vault Secret Token Renewal Loop Terminates Permanently on Network Error
• Location: `backend/src/config/vault.rs:60-110`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`, `error-handling`
• Description: The background task responsible for renewing HashiCorp Vault authentication tokens exits completely on any single network error, causing subsequent secret lookups to fail once the token expires.
• Remediation: Wrap Vault token renewal in an error-resilient loop with exponential backoff that continues retrying until the renewal deadline.

---

### [95/100] Secret Cache in Vault Service Stores Plaintext Passwords in Process Memory
• Location: `backend/src/config/vault.rs:120-160`
• Severity: Medium
• Labels: `backend`, `enhancement`, `auth-security`
• Description: Secrets fetched from Vault are cached in a standard `HashMap<String, String>` in process memory without zeroization on drop or protection against core dumps.
• Remediation: Wrap cached secret strings in `secrecy::SecretString` to prevent accidental logging and zeroize memory on eviction.

---

### [96/100] Input Sanitizer Stripper Misses Unicode Control and Zero-Width Characters
• Location: `backend/src/config/sanitize.rs:40-85`
• Severity: Low
• Labels: `backend`, `bug`, `data-validation`, `auth-security`
• Description: `sanitize_input` removes ASCII control characters and HTML tags, but does not strip Unicode bidirectional override characters (`U+202E`) or zero-width joiners that can be used for visual spoofing.
• Remediation: Add Unicode normalization (NFKC) and strip zero-width and bidirectional control codepoints in input sanitization routines.

---

### [97/100] Database Seed Script Fails Idempotency on Duplicate Unique Keys
• Location: `backend/src/db/seeds.rs:45-90`
• Severity: Medium
• Labels: `backend`, `bug`, `database`, `testing-infrastructure`
• Description: Executing the database seed script on an already populated database throws duplicate key violation errors rather than using `ON CONFLICT DO NOTHING / UPDATE` clauses.
• Remediation: Add `ON CONFLICT (name) DO NOTHING` clauses to all `INSERT` statements in database seeding logic.

---

### [98/100] Redis Health Check Reports Healthy When Node is in Read-Only Loading State
• Location: `backend/src/db/redis.rs:25-50`
• Severity: Medium
• Labels: `backend`, `bug`, `monitoring-observability`, `database`
• Description: The health probe checks Redis by executing `PING`, which succeeds even when the Redis replica is in loading state or read-only mode during cluster recovery.
• Remediation: Verify write capability or inspect Redis `INFO` persistence/replication status flags during readiness health checks.

---

### [99/100] Database Backup CLI Passes Password in Command Line Arguments
• Location: `backend/src/bin/backup.rs:180-220`
• Severity: High
• Labels: `backend`, `bug`, `auth-security`, `devops-deployment`
• Description: The backup binary invokes `pg_dump` via `std::process::Command` and passes database connection parameters including passwords via command-line arguments, exposing credentials to all users via `ps aux`.
• Remediation: Pass PostgreSQL credentials to `pg_dump` using `PGPASSWORD` environment variables or a temporary `.pgpass` file with restricted permissions.

---

### [100/100] CI Report Utility Exits With Zero Code on Benchmark Regressions
• Location: `backend/src/bin/ci_report.rs:15-35`
• Severity: Medium
• Labels: `backend`, `bug`, `testing-infrastructure`, `devops-deployment`
• Description: The `ci-report` CLI utility outputs JSON report status, but always exits with status code 0 (`Ok(())`) even when the report payload contains benchmark regressions or test failures, preventing CI pipeline gating.
• Remediation: Inspect report status in `ci-report` and exit with process code 1 when regressions or failures are detected.
