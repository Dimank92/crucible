#!/usr/bin/env bash
# ==============================================================================
# Script: publish_backend_batch2_50_issues.sh
# Purpose: Publish 50 additional backend issues to GitHub via `gh issue create`
# Target Repository: benelabs/crucible
# Series: [51/100] - [100/100]
# Note: Stellar Wave tag is strictly excluded as requested.
# ==============================================================================

set -euo pipefail

REPO="benelabs/crucible"

echo "=========================================================="
echo " Publishing 50 More Backend Issues to ${REPO}"
echo " Series: [51/100] to [100/100]"
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

  local full_title="[${index}/100] ${title}"
  local body="• Location: ${location}
• Severity: ${severity}
• Description: ${description}
• Remediation: ${remediation}"

  echo "==> Creating Issue ${index}/100: ${full_title} ..."

  gh issue create \
    --repo "${REPO}" \
    --title "${full_title}" \
    --body "${body}" \
    --label "${labels}"

  # Sleep briefly to avoid secondary GitHub API rate limiting
  sleep 1.5
}

# ------------------------------------------------------------------------------
# 51-60: Vulnerability Scanner, Benchmarking & Storage Optimizer
# ------------------------------------------------------------------------------

create_issue "51" \
  "Bytecode Vulnerability Scanner Relies on Naive Byte Pattern Matching" \
  "backend,bug,auth-security,data-validation" \
  "backend/src/services/vulnerability_scanner.rs:54-88" \
  "High" \
  "scan_bytecode flags reentrancy, unchecked arithmetic, and uninitialized storage by checking if arbitrary raw bytes (such as 0x54, which is the standard i32.wrap_i64 WASM opcode) appear anywhere in the binary, causing massive false positives on benign contracts." \
  "Replace raw byte slice matching with proper WebAssembly AST parsing using wasmparser to inspect instructions in their semantic context."

create_issue "52" \
  "Source Vulnerability Scanner Inspects Raw Strings Without AST Scoping" \
  "backend,bug,auth-security,data-validation" \
  "backend/src/services/vulnerability_scanner.rs:106-125" \
  "Medium" \
  "scan_source checks for substrings like unsafe, unwrap(), or expect( via raw substring search, triggering high-severity alerts on comments, test modules, and string literals." \
  "Use syn to parse Rust source code into an abstract syntax tree and evaluate unsafe blocks and unwraps only within active non-test function bodies."

create_issue "53" \
  "Vulnerability Scanner Fails to Report Source Code Line Numbers" \
  "backend,enhancement,api-architecture,error-handling" \
  "backend/src/services/vulnerability_scanner.rs:13-15" \
  "Low" \
  "All discovered vulnerabilities populate line_number as None, preventing API consumers and developers from locating the vulnerability in their contract source files." \
  "Extract line and column coordinates from syn or DWARF debug symbols during scanning and populate line_number in ScanResult."

create_issue "54" \
  "Contract Benchmark Historical Metrics Lost on Backend Restart" \
  "backend,bug,database,performance-optimization" \
  "backend/src/services/contract_benchmark.rs:11-20" \
  "Medium" \
  "Benchmark execution history is held purely in an in-memory VecDeque behind an RwLock with a hard limit of 50 entries, losing all historical regression baselines and latency trends across server restarts." \
  "Persist benchmark runs and baseline metrics into PostgreSQL tables with an indexed schema for historical time-series queries."

create_issue "55" \
  "Potential Division by Zero in Benchmark Aggregate Calculations" \
  "backend,bug,performance-optimization,error-handling" \
  "backend/src/services/contract_benchmark.rs:110-140" \
  "Low" \
  "When computing average duration or instruction metrics for benchmark runs with empty sample sets, the service risks producing NaN or infinite float values that break JSON serialization." \
  "Guard aggregate computation routines against empty sample collections and return explicit zeroed or optional summaries."

create_issue "56" \
  "Storage Rent Estimation Ignores Dynamic Ledger Configuration" \
  "backend,bug,api-architecture,data-validation" \
  "backend/src/services/contract_storage_optimizer.rs:25-36" \
  "Medium" \
  "The storage rent savings calculation uses hardcoded rent constants rather than fetching active network parameters from the Soroban RPC getLatestLedger endpoint." \
  "Query the active network ledger configuration parameters dynamically to calculate accurate storage rent projections."

create_issue "57" \
  "Storage Diff Generator XDR Key Stringification Causes Collision" \
  "backend,bug,data-validation,database" \
  "backend/src/services/contract_storage_optimizer.rs:50-59" \
  "Medium" \
  "Converting complex Soroban storage DataKeys to plain string keys via Debug formatting produces truncated or identical strings for distinct enum variants, corrupting storage diff comparisons." \
  "Serialize DataKeys to canonical base64-encoded XDR strings to guarantee unique and deterministic storage key identities."

create_issue "58" \
  "Synchronous Contract Invocation Logging Blocks Request Pipeline" \
  "backend,bug,performance-optimization,async-workers" \
  "backend/src/services/contract_call_logger.rs:40-80" \
  "High" \
  "Contract call logger executes direct database writes synchronously during HTTP request handling, causing request latencies to spike under high-frequency contract interaction loads." \
  "Buffer contract invocation logs in a bounded crossbeam/mpsc channel and flush them in batches via an asynchronous background worker."

create_issue "59" \
  "Contract Call Logs Table Lacks Automated Partitioning and TTL Retention" \
  "backend,enhancement,database,devops-deployment" \
  "backend/src/services/contract_call_logger.rs:85-120" \
  "Medium" \
  "The contract_call_logs table accumulates millions of invocation records indefinitely without time-based partitioning or automatic pruning policies, risking PostgreSQL disk space exhaustion." \
  "Implement PostgreSQL declarative monthly table partitioning on created_at and a scheduled retention cleaner."

create_issue "60" \
  "Contract Metadata Registry Overwrite Lacks Ownership Authorization" \
  "backend,bug,auth-security,database" \
  "backend/src/services/contract_registry.rs:30-65" \
  "High" \
  "Registering or updating contract metadata in the registry does not verify whether the caller holds administrative privileges or original deployer rights for the specified contract ID." \
  "Enforce deployer signature verification or organization-level permission checks prior to modifying contract registry records."

# ------------------------------------------------------------------------------
# 61-70: Monitoring, Analytics, Docs & Deduplication
# ------------------------------------------------------------------------------

create_issue "61" \
  "Contract Health Monitor Polling Interval Drifts Under Event Loop Pressure" \
  "backend,bug,async-workers,monitoring-observability" \
  "backend/src/services/contract_monitor.rs:40-85" \
  "Low" \
  "The contract health monitor uses sleep-based polling loops that accumulate scheduling drift when asynchronous tasks block the Tokio worker runtime." \
  "Replace manual sleep loops with tokio::time::interval configured with MissedTickBehavior::Skip for steady cadence monitoring."

create_issue "62" \
  "Contract Test Results Ingestion Lacks Cryptographic Checksum Validation" \
  "backend,bug,data-validation,auth-security" \
  "backend/src/services/contract_test_results.rs:25-60" \
  "Medium" \
  "Ingesting contract test run results accepts client-supplied pass/fail counts and coverage numbers without verifying an HMAC digest or test execution witness signature." \
  "Implement signed test result receipts verifying that test assertions were actually run in a verified Crucible test runner sandbox."

create_issue "63" \
  "Analytics Aggregator Computes Percentiles Using In-Memory Full Sorting" \
  "backend,bug,performance-optimization,database" \
  "backend/src/services/analytics_aggregator.rs:45-90" \
  "Medium" \
  "Percentile latencies (p50, p95, p99) are calculated by loading all raw invocation samples from the database into a heap-allocated vector and sorting in memory, exhausting memory on large query ranges." \
  "Offload percentile calculations to PostgreSQL percentile_cont or compute approximate percentiles using t-digest algorithms."

create_issue "64" \
  "Missing Pre-Aggregated Rollups for Historical Contract Activity" \
  "backend,enhancement,database,performance-optimization" \
  "backend/src/services/analytics_aggregator.rs:95-140" \
  "Medium" \
  "Historical dashboard analytics queries re-scan raw event tables for multi-day windows instead of querying daily and hourly pre-aggregated summary tables." \
  "Create hourly and daily rollup tables populated by a background cron job for instant historical analytics responses."

create_issue "65" \
  "Cumulative Business Metrics Counter Vulnerable to Integer Overflow" \
  "backend,bug,data-validation,performance-optimization" \
  "backend/src/services/business_metrics.rs:50-95" \
  "Low" \
  "Cumulative volume and stroop fee aggregators use standard u64 addition without checked or saturating arithmetic, risking integer overflow on long-running production environments." \
  "Upgrade cumulative metric accumulators to rust_decimal::Decimal or u128 with saturating_add."

create_issue "66" \
  "Compliance Sanction Verification Blocks Without Request Timeout" \
  "backend,bug,api-architecture,error-handling" \
  "backend/src/services/compliance.rs:30-70" \
  "High" \
  "Calling the external compliance / OFAC verification service relies on default HTTP client settings without an explicit request timeout, hanging caller threads if the upstream compliance API hangs." \
  "Wrap compliance checks in a strict 3-second timeout with fallback to temporary manual-review queues."

create_issue "67" \
  "Build Request Deduplication TTL Shorter Than Long Compilation Jobs" \
  "backend,bug,async-workers,performance-optimization" \
  "backend/src/services/dedup.rs:35-80" \
  "Medium" \
  "The build deduplication lock has a static 30-second TTL in Redis. Complex contract compilations taking longer than 30 seconds lose their deduplication key, triggering duplicate compilation runs." \
  "Extend deduplication locks dynamically using heartbeat renewals or compute TTL proportional to contract crate dependencies."

create_issue "68" \
  "Documentation Generator Crashes on Complex Unnamed Tuple Struct Returns" \
  "backend,bug,error-handling,data-validation" \
  "backend/src/services/doc_generator.rs:40-85" \
  "Medium" \
  "The OpenAPI / markdown documentation generator panics when parsing Soroban function return signatures containing deeply nested unnamed tuple structures." \
  "Implement robust recursive AST visitors in doc_generator that gracefully format tuple structs and complex enum variants."

create_issue "69" \
  "Unsanitized Markdown in Contract Documentation Generator Renders XSS" \
  "backend,bug,auth-security,api-architecture" \
  "backend/src/services/doc_generator.rs:90-130" \
  "High" \
  "Docstrings extracted from uploaded contract source code are rendered directly into OpenAPI description fields without HTML escaping, enabling Stored XSS attacks in Swagger UI." \
  "Pass all extracted docstrings through an HTML sanitization filter (such as ammonia) before embedding them into OpenAPI schema documentation."

create_issue "70" \
  "Error Recovery Manager Lacks Exponential Backoff for Persistent Failures" \
  "backend,bug,async-workers,error-handling" \
  "backend/src/services/error_recovery.rs:25-70" \
  "Medium" \
  "Tasks flagged for automatic error recovery are retried immediately on fixed intervals, rapidly consuming system resources when an external dependency is down." \
  "Implement truncated exponential backoff with jitter for all automated task recovery attempts."

# ------------------------------------------------------------------------------
# 71-80: gRPC, Metrics, Logging & Tracing
# ------------------------------------------------------------------------------

create_issue "71" \
  "gRPC Server Lacks Keepalive and Maximum Connection Age Configuration" \
  "backend,bug,api-architecture,performance-optimization" \
  "backend/src/services/grpc.rs:30-65" \
  "Medium" \
  "Tonic gRPC server initialization omits keepalive ping intervals, connection timeouts, and max connection age, leading to stale dead connections persisting through infrastructure load balancers." \
  "Configure http2_keepalive_interval, http2_keepalive_timeout, and max_connection_age on the Tonic Server builder."

create_issue "72" \
  "Unbounded Inbound Message Size on gRPC Contract Streaming Endpoint" \
  "backend,bug,auth-security,api-architecture" \
  "backend/src/services/grpc.rs:70-100" \
  "High" \
  "The gRPC contract deployment stream does not set max_decoding_message_size, allowing malicious clients to stream oversized byte buffers and trigger out-of-memory crashes." \
  "Enforce a 10MB max_decoding_message_size limit on the gRPC contract service."

create_issue "73" \
  "High Cardinality Route Dimensions in HTTP Metrics Prometheus Collector" \
  "backend,bug,monitoring-observability,performance-optimization" \
  "backend/src/services/http_metrics.rs:20-60" \
  "Medium" \
  "The HTTP metrics middleware includes raw request paths with un-parameterized dynamic UUID path segments in Prometheus metric labels, causing unbounded time-series cardinality explosion." \
  "Normalize URL paths to their route pattern templates (e.g. /api/v1/contracts/:id) before recording Prometheus metrics."

create_issue "74" \
  "Log Aggregator Channel Drops Warning Logs Under Burst Traffic" \
  "backend,bug,monitoring-observability,async-workers" \
  "backend/src/services/log_aggregator.rs:25-60" \
  "Medium" \
  "The log aggregation channel uses try_send with a small buffer, silently discarding warning and error log entries during traffic surges without recording dropped message counters." \
  "Increase channel capacity and maintain an atomic dropped_logs_total metric counter when buffer backpressure occurs."

create_issue "75" \
  "Synchronous Regular Expression Evaluation in Log Alert Engine" \
  "backend,bug,performance-optimization,async-workers" \
  "backend/src/services/log_alerts.rs:40-90" \
  "Medium" \
  "Evaluating log alert trigger conditions recompiles and evaluates regular expressions synchronously across every incoming log record, consuming significant CPU cycles." \
  "Pre-compile regex patterns with regex::RegexSet and evaluate log streams asynchronously on dedicated worker threads."

create_issue "76" \
  "Deployment Rollback Engine Does Not Restore Contract Temporary Storage" \
  "backend,bug,database,error-handling" \
  "backend/src/services/rollback.rs:35-85" \
  "High" \
  "The rollback service restores contract WASM bytecode and instance storage, but does not revert temporary or persistent storage entries modified by failed migration scripts." \
  "Include complete snapshotting of contract storage keyspaces prior to upgrades and restore all storage tiers on rollback."

create_issue "77" \
  "Security Scanner Does Not Validate WASM Section Header Offsets" \
  "backend,bug,auth-security,data-validation" \
  "backend/src/services/security_scanner.rs:40-85" \
  "High" \
  "Disassembling WASM binaries for unsafe instructions parses raw byte offsets without validating section lengths, causing panics on truncated or malformed WebAssembly files." \
  "Validate all WebAssembly section boundaries and payload sizes using wasmparser::Parser before attempting instruction extraction."

create_issue "78" \
  "System Metrics Collector Re-Initializes Hardware Probes on Every Scrape" \
  "backend,bug,monitoring-observability,performance-optimization" \
  "backend/src/services/sys_metrics.rs:60-110" \
  "Medium" \
  "Every scrape of /metrics re-creates a sysinfo::System instance and performs full disk and CPU hardware enumeration, adding tens of milliseconds of latency to metrics endpoints." \
  "Maintain a singleton System instance that refreshes periodically in the background (e.g. every 5 seconds) rather than synchronously on every HTTP scrape."

create_issue "79" \
  "Coverage Source Map Resolution Fails on Macro-Expanded Functions" \
  "backend,bug,testing-infrastructure,data-validation" \
  "backend/src/services/test_coverage.rs:50-95" \
  "Medium" \
  "Mapping test execution instruction counters back to source lines fails on contracts using procedural derive macros (like #[contractimpl]), resulting in misleading 0% line coverage reports." \
  "Incorporate macro expansion tracking and synthetic span mapping into coverage line resolution algorithms."

create_issue "80" \
  "OpenTelemetry OTLP Exporter Lacks Disk Buffer for Network Outages" \
  "backend,bug,monitoring-observability,error-handling" \
  "backend/src/services/tracing.rs:45-90" \
  "Low" \
  "When the OpenTelemetry collector endpoint is unreachable, OTLP span exports drop telemetry silently once in-memory ring buffers fill up." \
  "Implement persistent local spooling or disk-backed fallback buffers for telemetry spans during collector outages."

# ------------------------------------------------------------------------------
# 81-90: Background Workers, Task Queues & Schedulers
# ------------------------------------------------------------------------------

create_issue "81" \
  "Task Execution Worker Silently Discards Panics Without Diagnostic Trace" \
  "backend,bug,async-workers,error-handling" \
  "backend/src/workers/executor.rs:40-85" \
  "High" \
  "Background task execution handles tokio::task::JoinError by marking the job as failed, but does not extract or log the panic payload or backtrace, hindering debugging of worker crashes." \
  "Use std::panic::catch_unwind or join_error.into_panic() to extract and persist the panic message and backtrace into the failed job record."

create_issue "82" \
  "Missing Per-Task Timeout Enforcement in Background Worker Executor" \
  "backend,bug,async-workers,performance-optimization" \
  "backend/src/workers/executor.rs:90-130" \
  "High" \
  "Background jobs run indefinitely without an upper execution deadline. If a worker gets stuck in an infinite network wait or deadlocked mutex, worker concurrency slots are leaked permanently." \
  "Enforce a configurable per-job timeout (e.g., 5 minutes) via tokio::time::timeout on all executor tasks."

create_issue "83" \
  "Cache Pre-Warming Worker Queries Entire Contract Database Without Pagination" \
  "backend,bug,performance-optimization,database" \
  "backend/src/workers/cache_warm.rs:30-70" \
  "Medium" \
  "The cache warming worker executes a SELECT * FROM contracts query without a LIMIT or cursor, causing query timeouts and huge memory spikes on production databases with thousands of contracts." \
  "Implement cursor-based keyset pagination in the cache pre-warming worker."

create_issue "84" \
  "Job History Automated Pruning Locks Job Runs Table" \
  "backend,bug,database,async-workers" \
  "backend/src/workers/job_history.rs:35-75" \
  "Medium" \
  "Pruning expired job run records uses DELETE FROM job_runs WHERE finished_at < NOW() - INTERVAL '30 days' in a single unbounded query, taking exclusive table locks and blocking active job insertions." \
  "Execute deletions in bounded batches (e.g. DELETE in chunks of 1,000 with a sleep) or migrate job_runs to partitioned tables."

create_issue "85" \
  "Low Priority Task Queue Starvation Under Continuous High Priority Load" \
  "backend,bug,async-workers,performance-optimization" \
  "backend/src/workers/priority.rs:40-85" \
  "Medium" \
  "The priority task scheduler strictly drains the high-priority queue before inspecting medium or low queues. Sustained high-priority workloads indefinitely starve lower-priority background tasks." \
  "Implement a weighted fair queuing algorithm (e.g. 70% high, 20% medium, 10% low) to guarantee forward progress for lower priority jobs."

create_issue "86" \
  "Job Progress Notifications Skipped on Sub-Second Execution Stages" \
  "backend,enhancement,async-workers,api-architecture" \
  "backend/src/workers/progress.rs:30-65" \
  "Low" \
  "Background task progress updates only broadcast over WebSockets if the progress delta exceeds 10%, causing fast sub-tasks to jump from 0% straight to 100% without client visibility." \
  "Emit explicit phase transition events regardless of numerical percentage progress."

create_issue "87" \
  "Dead-Letter Queue Replay Lacks Jitter Leading to Synchronized Retry Waves" \
  "backend,bug,async-workers,performance-optimization" \
  "backend/src/workers/retry.rs:60-105" \
  "Medium" \
  "Bulk-replaying dead-letter jobs schedules all retries with identical delay timers, overwhelming downstream external APIs with synchronized thundering herd spikes." \
  "Introduce randomized jitter into bulk DLQ retry scheduling."

create_issue "88" \
  "Overly Permissive CORS Configuration in Non-Production Environments" \
  "backend,bug,auth-security,api-architecture" \
  "backend/src/config/cors.rs:15-35" \
  "Medium" \
  "cors.rs defaults to AllowOrigin::any() and allows credentials when environment is not set to production, exposing developer and staging setups to cross-origin credential theft." \
  "Explicitly reject AllowOrigin::any() in combination with allow_credentials(true) across all environments."

create_issue "89" \
  "Database Connection Acquire Timeout Too High Causing Request Stalls" \
  "backend,bug,database,performance-optimization" \
  "backend/src/config/database.rs:25-50" \
  "Medium" \
  "Database connection acquisition timeout is set to 30 seconds. When the database pool is exhausted, incoming HTTP requests stall for 30 seconds before failing, causing cascading frontend timeouts." \
  "Reduce connection acquire timeout to 3-5 seconds and return an immediate 503 Service Unavailable when the pool is saturated."

create_issue "90" \
  "Missing TCP Keepalive on PostgreSQL Connection Pool" \
  "backend,bug,database,devops-deployment" \
  "backend/src/config/database.rs:55-80" \
  "Medium" \
  "SQLx PgPool configuration does not configure TCP keepalive probes. Intermediate cloud firewalls and NAT gateways silently drop idle connections, causing subsequent queries to hang until TCP timeout." \
  "Configure PgPoolOptions with tcp_keepalive(Some(Duration::from_secs(60)))."

# ------------------------------------------------------------------------------
# 91-100: Redis, Server Lifecycle, Vault, Seeds & Utilities
# ------------------------------------------------------------------------------

create_issue "91" \
  "Redis Connection Manager Does Not Reconnect on Initial Startup Failure" \
  "backend,bug,database,error-handling" \
  "backend/src/config/redis.rs:20-55" \
  "High" \
  "If Redis is booting up or temporarily unreachable during backend startup, redis::Client::get_connection_manager fails immediately and aborts the entire application without retrying." \
  "Wrap initial Redis connection manager creation in an exponential retry loop with a configurable startup timeout."

create_issue "92" \
  "HTTP/2 Server Listener Lacks Max Concurrent Streams Bound" \
  "backend,bug,api-architecture,performance-optimization" \
  "backend/src/config/server.rs:30-65" \
  "High" \
  "The Axum HTTP listener does not restrict HTTP/2 max concurrent streams, allowing an attacker to open thousands of concurrent HTTP/2 streams over a single TCP connection (HTTP/2 Rapid Reset vulnerability)." \
  "Configure http2_max_concurrent_streams(Some(100)) on the Hyper server builder."

create_issue "93" \
  "Server Graceful Shutdown Does Not Drain Background Worker Jobs" \
  "backend,bug,async-workers,devops-deployment" \
  "backend/src/config/server.rs:70-110" \
  "High" \
  "Receiving a SIGTERM triggers Axum graceful shutdown for HTTP requests, but background workers and scheduled ingestion loops are terminated abruptly, leaving in-flight jobs in a half-committed state." \
  "Broadcast a shutdown cancellation token to all background workers and await worker termination before exiting the process."

create_issue "94" \
  "Vault Secret Token Renewal Loop Terminates Permanently on Network Error" \
  "backend,bug,auth-security,error-handling" \
  "backend/src/config/vault.rs:60-110" \
  "High" \
  "The background task responsible for renewing HashiCorp Vault authentication tokens exits completely on any single network error, causing subsequent secret lookups to fail once the token expires." \
  "Wrap Vault token renewal in an error-resilient loop with exponential backoff that continues retrying until the renewal deadline."

create_issue "95" \
  "Secret Cache in Vault Service Stores Plaintext Passwords in Process Memory" \
  "backend,enhancement,auth-security" \
  "backend/src/config/vault.rs:120-160" \
  "Medium" \
  "Secrets fetched from Vault are cached in a standard HashMap<String, String> in process memory without zeroization on drop or protection against core dumps." \
  "Wrap cached secret strings in secrecy::SecretString to prevent accidental logging and zeroize memory on eviction."

create_issue "96" \
  "Input Sanitizer Stripper Misses Unicode Control and Zero-Width Characters" \
  "backend,bug,data-validation,auth-security" \
  "backend/src/config/sanitize.rs:40-85" \
  "Low" \
  "sanitize_input removes ASCII control characters and HTML tags, but does not strip Unicode bidirectional override characters (U+202E) or zero-width joiners that can be used for visual spoofing." \
  "Add Unicode normalization (NFKC) and strip zero-width and bidirectional control codepoints in input sanitization routines."

create_issue "97" \
  "Database Seed Script Fails Idempotency on Duplicate Unique Keys" \
  "backend,bug,database,testing-infrastructure" \
  "backend/src/db/seeds.rs:45-90" \
  "Medium" \
  "Executing the database seed script on an already populated database throws duplicate key violation errors rather than using ON CONFLICT DO NOTHING / UPDATE clauses." \
  "Add ON CONFLICT (name) DO NOTHING clauses to all INSERT statements in database seeding logic."

create_issue "98" \
  "Redis Health Check Reports Healthy When Node is in Read-Only Loading State" \
  "backend,bug,monitoring-observability,database" \
  "backend/src/db/redis.rs:25-50" \
  "Medium" \
  "The health probe checks Redis by executing PING, which succeeds even when the Redis replica is in loading state or read-only mode during cluster recovery." \
  "Verify write capability or inspect Redis INFO persistence/replication status flags during readiness health checks."

create_issue "99" \
  "Database Backup CLI Passes Password in Command Line Arguments" \
  "backend,bug,auth-security,devops-deployment" \
  "backend/src/bin/backup.rs:180-220" \
  "High" \
  "The backup binary invokes pg_dump via std::process::Command and passes database connection parameters including passwords via command-line arguments, exposing credentials to all users via ps aux." \
  "Pass PostgreSQL credentials to pg_dump using PGPASSWORD environment variables or a temporary .pgpass file with restricted permissions."

create_issue "100" \
  "CI Report Utility Exits With Zero Code on Benchmark Regressions" \
  "backend,bug,testing-infrastructure,devops-deployment" \
  "backend/src/bin/ci_report.rs:15-35" \
  "Medium" \
  "The ci-report CLI utility outputs JSON report status, but always exits with status code 0 (Ok(())) even when the report payload contains benchmark regressions or test failures, preventing CI pipeline gating." \
  "Inspect report status in ci-report and exit with process code 1 when regressions or failures are detected."

echo "=========================================================="
echo " Finished publishing all 50 issues successfully!"
echo " Total published series: [51/100] to [100/100]"
echo "=========================================================="
