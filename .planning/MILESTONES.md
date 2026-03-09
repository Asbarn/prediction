# Milestones

## v1.0 MVP (Shipped: 2026-02-24)

**Phases completed:** 13 phases, 36 plans
**Lines of Rust:** 22,751
**Timeline:** 4 days (2026-02-21 to 2026-02-24)
**Commits:** 160
**Audit:** 46/46 requirements satisfied, 13/13 phases verified, 8/8 E2E flows

**Key accomplishments:**
- Three-venue feed infrastructure (Deribit WebSocket, Polymarket CLOB, Kalshi) with unified MarketSnapshot channel, reconnection supervisors, and staleness detection
- Black-76 options pricing engine with IV solver (Newton-Raphson + Brent), vol surface interpolation, call spread replication for digital payoffs, and Greeks
- Cross-asset arbitrage signal detection between options-implied probabilities and prediction market prices with configurable dynamic thresholds
- Cost-adjusted spread computation incorporating settlement basis risk premium, venue-specific transaction fees, slippage, and staleness protection
- Deterministic replay from recorded feeds, Prometheus metrics exporter, HTTP health endpoint, paper trade P&L tracking, and stable JSONL schema
- Config-driven event registry mapping cross-venue instruments with quantified settlement basis risk scores and contract lifecycle management

**Tech debt carried forward:** 13 non-blocking items (iv_spread always 0.0, expired test instrument, empty Kalshi default config, options book depth hardcoded 0)

---


## v1.1 Paper Trading Validation (Shipped: 2026-02-26)

**Phases completed:** 4 phases, 11 plans
**LOC delta:** +14,943 lines (32,631 LOC Rust total)
**Timeline:** 5 days (2026-02-21 to 2026-02-26)
**Commits:** 47
**Requirements:** 25/25 satisfied

**Key accomplishments:**
- Failure alerting with 4 alert types (feed silence, partial coverage, signal gap, stage liveness), Prometheus gauges, and cooldown-based dedup preventing log spam
- Atomic checkpoint persistence with JSONL replay recovery — paper trades, daily rollups, and signal analysis accumulators survive restarts
- Three-venue settlement outcome tracking (Deribit delivery prices, Kalshi resolution results, Polymarket Gamma API inference) with 4-tier polling cadence and startup backfill
- Full settlement integration into PaperTradeTracker with per-leg P&L, cross-venue divergence detection, and daily-rotating settlement JSONL
- Signal analysis tooling computing hit rate, cost-adjusted edge, false positive rate, time-to-convergence, and threshold effectiveness across PassedBoth/PassedStaticOnly/Filtered categories
- Filtered signal tracking channel with hypothetical hit rate correlation to answer "are thresholds too aggressive?"

**Tech debt carried forward:** 13 non-blocking items from v1.0 (unchanged)

---


## v1.2 Automated Event Management (Shipped: 2026-02-27)

**Phases completed:** 4 phases, 8 plans
**LOC delta:** +2,122 lines (34,753 LOC Rust total)
**Timeline:** 2 days (2026-02-26 to 2026-02-27)
**Commits:** 16
**Requirements:** 15/15 satisfied

**Key accomplishments:**
- Production-safe venue discovery polling with shared VenueRateLimiter instances, consecutive-absence guards preventing false expirations, and batched TOML writes eliminating race conditions
- Polymarket Gamma API structured discovery with question text parsing (3 BTC price patterns), ExpiryConfidence scoring, and unified Vec<DiscoveredInstrument> type across all three venues
- Cross-venue fuzzy matching via FuzzyMatchKey (asset/strike/direction) with configurable expiry tolerance window, handling Deribit Friday and Kalshi end-of-month expiry differences
- Proposal workflow with WARN-level structured tracing logs, Prometheus proposals_pending gauge and proposals_total counter, and atomic TOML writes preserving formatting
- Approved-mapping validation on config reload (venue count >= 2, expiry not past) plus async instrument-activity warnings gated behind discovery data availability
- Event archival from events.toml to events_archive.toml with Retired lifecycle status, automatic unapproved-candidate cleanup, and full pipeline running as periodic background task in ContractLifecycleManager

**Tech debt carried forward:** 13 non-blocking items from v1.0 (unchanged) + 2 low-severity items (unused exact-match functions preserved for backward compat, expiry_confidence TOML field is write-only)

---


## v1.3 Live Subscription Management (Shipped: 2026-02-28)

**Phases completed:** 4 phases, 7 plans
**LOC delta:** +827 lines (35,580 LOC Rust total)
**Timeline:** 2 days (2026-02-27 to 2026-02-28)
**Commits:** 31
**Requirements:** 14/14 satisfied
**Audit:** 14/14 requirements, 4/4 phases, 14/14 integration, 5/5 E2E flows

**Key accomplishments:**
- SubscriptionManager with per-venue HashSet diff reconciliation, watch channel push, and Notify-based ordering guarantee ensuring registry refresh completes before subscription reads
- All three venue supervisors (Deribit, Polymarket, Kalshi) dynamically subscribe/unsubscribe via watch::Receiver without restart -- operator approves in events.toml, system responds within one config reload cycle
- Prometheus subscription metrics (active gauge per venue, activation/removal counters) and dry-run reconciliation mode for safe operator testing
- Stale state cleanup after unsubscribe: 5 stateful engines evict entries via mpsc cleanup channels, preventing phantom signals from stale data paired with live data
- iv_spread populated from actual IV solver bid-ask spread (was always 0.0), options book depth config-driven (was hardcoded 0), Kalshi staleness computed from exchange_timestamp age (was always false)
- Zero new crate dependencies (continues v1.1/v1.2 pattern)

**Tech debt carried forward:** 10 non-blocking items from v1.0 (3 behavior-changing items fixed in v1.3) + 2 low-severity from v1.2 + 3 non-critical from v1.3 audit (stale comment, unused CleanupEvent field, dead PipelineHandles field)

---


## v1.4 Analysis Tooling (Shipped: 2026-03-02)

**Phases completed:** 4 phases, 7 plans, 11 tasks
**LOC delta:** +927 lines (36,507 LOC Rust total)
**Timeline:** 1 day (2026-02-28)
**Commits:** 11
**Requirements:** 12/12 satisfied

**Key accomplishments:**
- Shared analysis infrastructure: pure statistics module (mean, stddev, percentile, wilson_ci, skewness, kurtosis), tolerant JSONL loader with date-range file enumeration, dual-mode output (table/JSON) with comfy-table rendering
- `spread-analytics` CLI: distribution summary (net/gross with p5-p95), 24-row hourly breakdown revealing opportunity clustering, venue-pair analysis with directional detail, --by-event and --output json support
- `signal-scoring` CLI: hit rate with Wilson CIs at 95%/99%, cost-adjusted edge t-test (t-stat, p-value, CI), per-trade and annualized Sharpe with PSR (Bailey & Lopez de Prado), max drawdown with recovery dates
- 13 E2E golden-value integration tests (6 spread + 7 signal) proving computation correctness against hand-verified expected values with epsilon tolerances
- Pure-function architecture: all computation functions accept slices, return Options, no side effects; deterministic BTreeMap bucketing for ordered output

**Tech debt carried forward:** Same as v1.3 (no new items)

---


## v1.5 Derive.xyz Venue Integration (Shipped: 2026-03-06)

**Phases completed:** 4 phases, 10 plans, 20 tasks
**LOC delta:** +8,164 lines (39,176 LOC Rust total)
**Timeline:** 2 days (2026-03-04 to 2026-03-06)
**Commits:** 43
**Requirements:** 18/18 satisfied

**Key accomplishments:**
- Venue::Derive enum variant with full type system integration across 14+ files, DeriveConfig/DeriveMapping structs, and [derive] venues.toml section — zero todo!() placeholders
- Derive WebSocket feed with snapshot-only book model (~100ms updates), ticker_slim parsing with abbreviated single-letter keys, USDC price pass-through normalization, and JSONL recording
- 4-venue live pipeline with DeriveSupervisor/DeriveProcessor wired into run_live_multi_venue via 7-step block pattern (health, cancel, recording, rate-limiter, supervisor, processor, forward) with crash isolation via child CancellationToken
- SubscriptionManager extended from 3-venue to 4-venue with Derive HashSet diff reconciliation, watch channel push, CleanupEvent.derive_instruments populated from actual diff, and subscription metrics with venue=derive label
- Prometheus observability: feed_latency_ms, feed_messages_total, subscription_active/activations/removals with venue=derive, and feed_reconnections_total counter benefiting all 4 venues
- REST-based discover_derive() with POST to Lyra's /public/get_instruments, Decimal strike parsing, epoch-to-date conversion, cross-venue fuzzy matching, and lifecycle integration with configurable 300s poll interval

**Tech debt carried forward:** Same as v1.4 (no new items)

---


## v1.6 Production Deployment (Shipped: 2026-03-09)

**Phases completed:** 6 phases, 12 plans
**LOC delta:** +1,825 lines (42,732 LOC Rust + 499 LOC CDK TypeScript + 1,093 LOC Grafana provisioning)
**Timeline:** 2 days (2026-03-07 to 2026-03-08)
**Requirements:** 24/24 satisfied
**Audit:** 24/24 requirements, 6/6 phases, 24/24 integration, 3/3 E2E flows

**Key accomplishments:**
- AWS CDK Infrastructure as Code: single `cdk deploy` provisions VPC, security groups, EC2, IAM, EBS, CloudWatch, Secrets Manager, AMP, and ECR import (499 LOC TypeScript)
- Production EC2 bootstrap with user-data installing Docker, systemd service, secrets injection from Secrets Manager, auto-restart on failure, and graceful SIGTERM shutdown (exit code 0)
- CloudWatch log aggregation via conditional JSON stdout layer and awslogs Docker driver, plus CloudWatch Agent for EC2 host metrics (CPU, memory, disk)
- Prometheus sidecar scraping 80+ app metrics, remote_write to Amazon Managed Prometheus with SigV4, self-hosted Grafana OSS as visualization layer (user-approved deviation from AMG)
- GitLab CI/CD pipeline: automated test, build (cargo-chef 3-stage Dockerfile), push to ECR, deploy via SSM Send-Command with health check verification -- zero SSH required
- 4 Grafana operational dashboards (Feed Health, Signal Quality, Paper Trade P&L, System Health) + 3 alert rules provisioned via CDK S3 asset

**Tech debt carried forward:** 4 non-critical items (stdout_json not codified in user-data, Grafana open to 0.0.0.0/0 with default creds, dashboard count wording discrepancy, removed contact-points.yml due to SMTP crash)

---

