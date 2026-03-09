# Prediction Market Arbitrage System

## What This Is

A production-grade cross-venue arbitrage signal generator in Rust that detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options markets (Deribit, Derive). Compares prediction market binary contract prices against options-implied probabilities derived via Black-76 pricing with call spread replication, generates trading signals when spreads exceed cost-adjusted thresholds, tracks settlement outcomes for signal validation, computes statistical evidence of signal quality, automatically discovers new cross-venue instrument matches with operator-approved proposals, dynamically manages feed subscriptions as instruments are approved or retired, and provides offline CLI analysis tools for statistically rigorous go/no-go decisions. Deployed to production-hardened AWS infrastructure with CDK, CI/CD, Prometheus/Grafana monitoring, and CloudWatch logging. Built as a single-binary service for a solo trader with 42,732 lines of Rust and full deterministic replay capability.

## Core Value

Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## Requirements

### Validated

- v1.0 Deribit WebSocket feed with order book maintenance and JSON-RPC 2.0 parsing
- v1.0 Polymarket CLOB WebSocket feed with probability-space order books
- v1.0 Kalshi feed (WebSocket) normalized to same schema
- v1.0 Unified MarketSnapshot bus via bounded async channels
- v1.0 Raw feed recording to line-delimited JSON with timestamps
- v1.0 Event registry mapping equivalent instruments across venues (TOML-driven)
- v1.0 Settlement basis analyzer quantifying expiry/oracle/resolution differences
- v1.0 IV solver (Newton-Raphson/Brent) for Black-76 options pricing
- v1.0 Probability extractor: N(d2), call spread replication, smile interpolation
- v1.0 Greeks calculator (delta, vega, theta) for position monitoring
- v1.0 Spread calculator with cost, slippage, funding, and basis risk adjustments
- v1.0 Staleness detection rejecting stale data per configurable threshold
- v1.0 Signal generation with dynamic edge thresholds
- v1.0 Continuous spread logging and periodic aggregate metrics
- v1.0 Prometheus metrics exporter
- v1.0 Structured logging via tracing with JSON output and correlation IDs
- v1.0 Mock/replay data layer for development and backtesting
- v1.0 Config-driven TOML for all parameters
- v1.0 Graceful degradation on feed drops
- v1.0 Deterministic replay from recorded feeds
- v1.0 HTTP /health endpoint
- v1.0 Paper trade P&L tracking
- v1.0 Contract lifecycle management with expiry rolls
- v1.0 Per-venue heartbeat monitoring and reconnection supervisors
- v1.1 Settlement outcome tracking from Deribit, Kalshi, and Polymarket with 4-tier polling
- v1.1 Signal analysis tooling (hit rate, edge, false positive rate, time-to-convergence, threshold effectiveness)
- v1.1 Failure alerting for degraded states (feed silence, partial coverage, signal gap, stage liveness)
- v1.1 File-based state persistence with atomic checkpoints and JSONL replay recovery

- v1.2 Automated three-venue market discovery (Polymarket Gamma API, Deribit, Kalshi) with shared rate limiters and absence guards
- v1.2 Cross-venue fuzzy matching (asset/strike/direction) with configurable expiry tolerance and confidence scoring
- v1.2 Automatic proposal writing to events.toml (approved = false) with structured WARN logging and Prometheus metrics
- v1.2 Approved-mapping validation on config reload (venue count, expiry, instrument activity)
- v1.2 Expired event archival to events_archive.toml with Retired lifecycle status
- v1.2 Unapproved candidate auto-cleanup and full pipeline as periodic background task

- v1.3 Dynamic feed subscription for newly approved instruments without restart (reconnect-based, all 3 venues)
- v1.3 Dynamic feed unsubscription for expired/retired instruments with stale state cleanup across 5 engines
- v1.3 Config-change-driven subscription reconciliation with per-venue HashSet diff, Notify ordering, and structured tracing
- v1.3 Prometheus subscription metrics (active gauge, activation/removal counters per venue) and dry-run reconciliation mode
- v1.3 iv_spread populated from actual IV solver bid-ask spread, options book depth config-driven, Kalshi staleness from exchange_timestamp

- v1.4 Shared analysis infrastructure: stats module (mean, stddev, percentile, wilson_ci, skewness, kurtosis), tolerant JSONL loader with date-range file enumeration, dual-mode output (table/JSON)
- v1.4 `spread-analytics` CLI: distribution summary, 24-row hourly breakdown, venue-pair analysis with --by-event and --output json support
- v1.4 `signal-scoring` CLI: hit rate with Wilson CIs, cost-adjusted edge t-test, Sharpe/PSR, max drawdown with recovery dates, --by-event and --output json support
- v1.4 13 E2E golden-value integration tests proving computation correctness for both CLIs

- v1.5 Derive.xyz WebSocket feed with snapshot-only book model, ticker_slim parsing, USDC pass-through normalization, and auto-reconnection supervisor
- v1.5 Derive options-implied probability extraction via same Black-76/call spread pipeline as Deribit (venue-gated price conversion)
- v1.5 4-venue live pipeline: Derive snapshots flow through run_live_multi_venue to SpreadEngine/SignalEngine/PaperTradeTracker without downstream changes
- v1.5 Dynamic subscription support for Derive instruments via SubscriptionManager with HashSet diff, watch channels, and CleanupEvent population
- v1.5 REST-based Derive BTC options discovery with cross-venue fuzzy matching and lifecycle integration (300s poll interval)

- v1.6 AWS CDK infrastructure as code: single `cdk deploy` provisions VPC, security groups, EC2, IAM, EBS, CloudWatch log group, Secrets Manager, AMP workspace, and ECR import
- v1.6 Production EC2 bootstrap with user-data, systemd service, fetch-secrets.sh from Secrets Manager, auto-restart on failure, and graceful SIGTERM shutdown
- v1.6 CloudWatch log aggregation via conditional JSON stdout layer and awslogs Docker driver, plus CloudWatch Agent for EC2 host metrics
- v1.6 Prometheus sidecar scraping 80+ app metrics, remote_write to Amazon Managed Prometheus with SigV4, self-hosted Grafana OSS
- v1.6 GitLab CI/CD pipeline: automated test, Docker build with cargo-chef caching, ECR push, deploy via SSM Send-Command with health check
- v1.6 4 Grafana operational dashboards (Feed Health, Signal Quality, Paper Trade P&L, System Health) + 3 alert rules provisioned via CDK S3 asset

### Active

## Current Milestone: v1.7 Prediction Market Signal Pipeline

**Goal:** Get Polymarket data flowing in production and generate actual cross-asset arbitrage signals (options-implied probability vs prediction market price).

**Target features:**
- Investigate and fix Polymarket WebSocket connectivity from AWS EC2
- Generalize spread engine beyond Polymarket+Kalshi hardcoding to support single prediction market vs options-implied probability
- Generalize signal engine to work with any single prediction market venue
- End-to-end production verification of signal generation pipeline

### Out of Scope

- Order execution / trade placement -- v2 after paper trading validation
- Venue API authentication for private/trading endpoints -- v2
- Real-time P&L and position tracking -- v2
- Risk limits engine and kill switch -- v2
- Margin monitoring -- v2
- Multi-asset support (ETH, SOL) -- after BTC binary events validated; architecture supports via config
- UI / dashboard -- solo trader monitors via Grafana dashboards and Prometheus metrics
- AI/ML signal prediction -- arbs are event-driven, not pattern-driven
- Sub-millisecond latency -- arb windows are minutes-to-hours
- NLP/ML-based Polymarket question parsing -- regex sufficient for predictable BTC price patterns
- Automatic approval of high-confidence matches -- human gate is non-negotiable safety mechanism
- Database-backed event store -- TOML sufficient at dozens-to-hundreds of entries, human-readable and git-trackable
- Real-time TUI dashboard -- Prometheus + Grafana covers live monitoring; analysis tools are for offline evaluation
- Database backend (SQLite/DuckDB) for analysis -- JSONL sufficient at current scale; Vec<T> faster for expected volumes
- Full backtesting engine -- settled data is stronger evidence than simulated backtests
- Terminal charting -- JSON output + external tools preferred
- ECS/Fargate deployment -- massive complexity for one container; Docker Compose on EC2 is correct abstraction
- Multi-AZ / auto-scaling -- single instance by design; downtime tolerance is minutes
- Blue/green deployments -- solo trader tolerates 30-second restart
- Kubernetes / EKS -- orchestration overkill for single container
- Self-hosted Prometheus + Grafana on same EC2 -- hosting monitoring on production instance defeats purpose; AMP used as managed store

## Context

**Shipped v1.6 Production Deployment** (2026-03-09) with 42,732 LOC Rust + 499 LOC CDK TypeScript + 1,093 LOC Grafana provisioning across 39 phases (7 milestones).
Tech stack: Rust (2024 edition), tokio, rust_decimal, serde, axum, metrics/prometheus, statrs, comfy-table, tracing, strsim. Infrastructure: AWS CDK (TypeScript), GitLab CI, Docker, systemd, Prometheus, Grafana OSS, Amazon Managed Prometheus, CloudWatch.
4 venues operational: Deribit (WebSocket + REST settlement + discovery), Polymarket (CLOB WebSocket + Gamma API discovery), Kalshi (WebSocket + REST + discovery), Derive (WebSocket + REST discovery).
Dynamic subscription: SubscriptionManager with 4-venue reconciliation, watch channels to supervisors, Notify ordering, dry-run mode, and stale state cleanup across 5 engines.
Automated event management: four-venue discovery with fuzzy matching, confidence-scored proposals, approved-mapping validation, expired event archival, and periodic background pipeline.
Settlement tracking: 3 venue resolution checkers with 4-tier polling cadence, startup backfill, and auto-settlement (Derive settlement deferred to future).
Analysis tooling: `spread-analytics` CLI (distribution, hourly, venue-pair) and `signal-scoring` CLI (hit rate, Sharpe, PSR, drawdown, edge t-test) with E2E golden-value tests.
Production infrastructure: CDK-managed AWS (VPC, EC2, IAM, EBS, Secrets Manager, CloudWatch, AMP), GitLab CI/CD pipeline (test, build, deploy via SSM), Prometheus sidecar + Grafana OSS with 4 dashboards and 3 alert rules.

**System status:** Fully operational in production. System runs unattended on AWS EC2 with automated CI/CD deployments, Prometheus/Grafana monitoring, CloudWatch logging, and Secrets Manager credential injection. Paper trading with 4-venue capability, self-managing event lifecycle, self-managing feed subscriptions, and offline analysis CLIs.

**Next priority:** v1.7 Prediction Market Signal Pipeline -- get Polymarket data flowing and generate cross-asset arbitrage signals.

**Known tech debt:** 4 non-critical items from v1.6 (stdout_json not codified in user-data, Grafana open to 0.0.0.0/0, dashboard count wording, removed contact-points.yml). See MILESTONES.md for full history.

## Constraints

- **Language**: Rust (latest stable, 2024 edition)
- **Async runtime**: tokio
- **Decimal arithmetic**: `rust_decimal` for all prices and probabilities
- **Deployment**: Docker Compose on AWS EC2, deployed via GitLab CI/CD + SSM
- **Infrastructure**: AWS CDK (TypeScript), single-stack single-instance
- **Deribit API**: 20 req/s rate limit on private endpoints
- **Polymarket**: On-chain (Polygon) -- gas, wallet, approvals matter for v2 execution
- **Kalshi**: US-regulated, different API semantics and fee structures

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Paper trading before execution | Validate signal quality before risking capital | v1.0 Validated -- system ready for paper trading |
| BTC-only initially | Highest liquidity across all venues | v1.0 Validated -- BTC pipeline complete |
| JSONL feed recording | Easy to grep/parse; human-readable for debugging | v1.0 Validated -- stable schema, golden tests |
| Call spread replication as primary digital pricing | More robust than naive N(d2) under volatility skew | v1.0 Validated -- primary method with confidence scoring |
| Config-driven via TOML | No recompilation for parameter changes during tuning | v1.0 Validated -- all parameters configurable |
| Mock + real data layers | Development without API keys; recordings become replay corpus | v1.0 Validated -- deterministic replay operational |
| 9-phase structure (expanded from 6) | Clearer delivery boundaries (reliability separate from connection, etc.) | v1.0 Validated -- clean incremental delivery |
| Deribit feed first | Proves pipeline architecture before multi-venue complexity | v1.0 Validated -- architecture held through 3 venues |
| Prediction market arb before cross-asset | Validates pipeline with simpler math before Black-76 | v1.0 Validated -- both spread engines operational |
| Gamma omitted from Greeks | User decision: delta/vega/theta sufficient for paper trading | v1.0 Accepted |
| Flat extrapolation for vol surface | Returns boundary IV rather than None for extreme strikes | v1.0 Validated -- graceful degradation |
| Non-blocking try_send for secondary engines | Primary engine (SpreadEngine) blocking, others best-effort | v1.0 Validated -- no pipeline stalls |
| BasisRiskCache with try_read | Never blocks engine hot path; zero premium on lock contention | v1.0 Validated -- no measurable latency impact |
| Zero new crate dependencies for v1.1 | All features built on existing dependency tree | v1.1 Validated -- no supply chain growth |
| Alerting first in v1.1 build order | Monitors running during rest of development | v1.1 Validated -- caught no issues during dev |
| AtomicI64 for pipeline liveness timestamps | Lock-free reads in hot path vs Mutex<DateTime> | v1.1 Validated -- zero contention |
| VenueChecker enum dispatch (not async-trait) | Zero new dependencies for venue settlement checking | v1.1 Validated -- clean pattern |
| Checkpoint version as u32 (not semver) | Compact schema evolution with backward-compatible serde(default) | v1.1 Validated -- v1 through v4 forward-compatible |
| Filtered signals via try_send (best-effort) | Avoid backpressure on SpreadEngine hot path | v1.1 Validated -- no pipeline stalls |
| String parsing over regex for Polymarket questions | 3 predictable BTC price patterns; regex adds complexity | v1.2 Validated -- parses all current formats |
| endDateIso as authoritative expiry source | Question text dates vary; API field is canonical | v1.2 Validated -- reliable across venues |
| FuzzyMatchKey (asset/strike/direction) for matching | Expiry checked separately against tolerance window | v1.2 Validated -- handles cross-venue expiry differences |
| Batched TOML writes per poll cycle | Prevents write/file-watcher race conditions | v1.2 Validated -- atomic write pattern |
| N consecutive absences before expiry | Prevents false expirations from partial API responses | v1.2 Validated -- configurable threshold |
| approved = false as non-negotiable safety gate | Human approval required before capital allocation | v1.2 Validated -- core safety mechanism |
| Live subscription management deferred to v1.3 | Restart-on-approval is acceptable for paper trading | v1.2 Accepted -- shipped in v1.3 |
| Single new dependency (strsim) for v1.2 | Already compiled transitively via clap_builder | v1.2 Validated -- zero supply chain growth |
| Archive-then-remove safety pattern | Archive file written atomically before entries removed | v1.2 Validated -- no data loss risk |
| Reconnect-based subscription for all 3 venues | Uniform approach avoids per-venue protocol differences | v1.3 Validated -- all venues respond to watch channel updates |
| tokio::sync::watch for instrument list push | Latest-value semantics; supervisors always get current list | v1.3 Validated -- clean pattern |
| tokio::sync::Notify for registry-before-subscription ordering | Ensures registry refresh completes before reconciliation reads | v1.3 Validated -- no race conditions |
| Zero new crate dependencies for v1.3 | Continues v1.1/v1.2 pattern of building on existing deps | v1.3 Validated -- zero supply chain growth |
| Tech debt sweep in separate final phase | Clean bisectability; behavior changes isolated from subscription work | v1.3 Validated -- clean separation |
| Vec<mpsc::Sender> for cleanup channels | Fixed number of consumers (5 engines); no broadcast needed | v1.3 Validated -- simple, correct |
| Registry-retain pattern for stale state cleanup | Engines read active_approved(), retain matching entries only | v1.3 Validated -- authoritative cleanup source |
| Decimal for financial mean, f64 for statistical functions | Precision boundary: financial values in Decimal, statistical in f64 | v1.4 Validated -- clean separation |
| Synchronous fn main() for CLI binaries | No tokio runtime needed for batch analysis tools | v1.4 Validated -- simpler dependency chain |
| 365.25-day year for Sharpe annualization | Prediction markets trade 24/7, not 252 stock trading days | v1.4 Validated -- correct for asset class |
| PSR via Bailey & Lopez de Prado formula | Accounts for non-normal return distribution (skewness/kurtosis) | v1.4 Validated -- robust statistical inference |
| statrs for t-distribution and normal CDF | Standard library for statistical distributions | v1.4 Validated -- correct p-values |
| Generated JSONL fixtures (not hand-written) | Prevents schema drift between struct definitions and test data | v1.4 Validated -- reliable E2E tests |
| Snapshot-only book model for Derive | No delta reconciliation needed; simpler than Deribit | v1.5 Validated -- clean feed implementation |
| ticker_slim over deprecated ticker channel | Derive deprecated ticker; ticker_slim uses abbreviated keys | v1.5 Validated -- discovered via live API probe |
| No k256/auth for v1.5 (public channels only) | Trading/private endpoints deferred to v2 execution | v1.5 Validated -- zero auth complexity |
| USDC price pass-through (no inverse transform) | Derive quotes in USDC linear; Deribit BTC-inverse needs transform | v1.5 Validated -- venue-gated in PricingEngine |
| POST for Derive REST discovery (not GET) | Derive API returns 405 on GET requests | v1.5 Validated -- confirmed at runtime |
| Epoch expiry auto-detect (seconds vs millis) | Threshold at 10 billion handles both formats | v1.5 Validated -- robust parsing |
| feed_reconnections_total as venue-generic metric | Benefits all 4 venues, not Derive-specific | v1.5 Validated -- clean observability |
| Copy-and-adapt Deribit feed stack pattern | 7-step pipeline block identical across venues | v1.5 Validated -- consistent architecture |
| Single CDK stack (no multi-stack) | Single-developer project; simplicity over isolation | v1.6 Validated -- clean deploy |
| ECR imported by name not created | Preserves existing image history | v1.6 Validated -- no duplicate repos |
| No NAT gateway (public subnet only) | Saves $32/month; acceptable for single instance | v1.6 Validated -- cost-effective |
| Secrets injected via .env from Secrets Manager | Simpler than mounted volume for flat key-value pairs | v1.6 Validated -- clean separation |
| systemd manages restart, docker restart="no" | Single responsibility; systemd handles lifecycle | v1.6 Validated -- clean restart behavior |
| Self-hosted Grafana OSS replaces AMG | AMG requires IAM Identity Center (SSO) subscription | v1.6 Validated -- equivalent functionality via SigV4 |
| SigV4AuthType=default for instance role chain | No static credentials; uses EC2 instance role | v1.6 Validated -- secure credential flow |
| IMDSv2 hop limit=2 for Docker containers | Required for containers to reach instance metadata | v1.6 Validated -- Prometheus and Grafana can auth |
| SSM Send-Command for CI deploy (not SSH) | No SSH keys in CI; scoped IAM permissions | v1.6 Validated -- secure deployment |
| S3 Asset for Grafana provisioning | Dashboard JSON exceeded 16KB user-data limit | v1.6 Validated -- more maintainable than heredocs |
| cargo-chef 3-stage Dockerfile | Dependency layer caching reduces rebuild time | v1.6 Validated -- fast incremental builds |

---
*Last updated: 2026-03-09 after v1.7 milestone start*
