# Phase 16: Settlement Outcome Tracking - Research

**Researched:** 2026-02-24
**Domain:** Venue settlement API integration, position settlement lifecycle, cross-venue divergence tracking
**Confidence:** MEDIUM-HIGH

## Summary

Phase 16 adds a dedicated `SettlementMonitor` tokio task that polls each venue's REST API to detect when prediction market events and options expirations have resolved. The three venues have fundamentally different settlement mechanisms: Deribit uses a TWAP-based delivery price published to a public endpoint at 08:00 UTC on expiry day; Kalshi exposes market status transitions (active -> closed -> determined -> finalized) with explicit `result` and `settlement_value_dollars` fields; Polymarket relies on the Gamma API where `closed=true` plus outcome token prices locking to 0.00/1.00 indicates resolution, with `umaResolutionStatuses` tracking the UMA oracle dispute pipeline.

The architecture follows the AlertMonitor pattern already established in Phase 14: a long-running tokio task with `select! biased` for cancellation, periodic polling intervals, and trait-based venue abstractions for testability. All new types (SettlementOutcome, ResolutionResult, SettledLeg, SettlementRecord) are pure Rust structs using the existing dependency set (serde, rust_decimal, chrono, reqwest). No new crate dependencies are required.

**Primary recommendation:** Build a trait `ResolutionChecker` with three implementations (Deribit, Kalshi, Polymarket) that share the existing `reqwest::Client` and rate limiters. The SettlementMonitor orchestrates polling with a four-tier cadence (aggressive -> patient -> lazy -> timeout), communicates SettlementOutcomes to PaperTradeTracker via an `mpsc` channel, and extends the Phase 15 CheckpointState with settlement tracking fields.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Polling Strategy:**
- Three-tier trigger model: Deribit deterministic (08:00 UTC on expiry), prediction markets lazy-then-aggressive anchored to Deribit settlement, fixed moderate cadence for Poly-vs-Kalshi only pairs.
- Four-phase polling cadence after trigger: Aggressive (0-4h, 2-5 min), Patient (4-96h, 15-30 min), Lazy (96h-7d, 2-4h), Timeout at 7 days.
- Timeout is configurable per venue in TOML. `resolution_timeout` is a distinct state, never contaminates signal quality metrics.

**Architecture:**
- Dedicated SettlementMonitor tokio task following AlertMonitor pattern from Phase 14.
- Data flow: ContractLifecycleManager -> SettlementMonitor (expiry awareness) -> venue API polling -> SettlementOutcome on channel -> PaperTradeTracker (position settlement).
- Trait-based venue client for testability. Each venue implements a resolution check trait returning `ResolutionResult`.
- Shared rate limiter from Phase 3 (Arc<RateLimiter> per venue). No independent rate budget.

**Resolution Logic:**
- Authoritative API status, not price inference. Polymarket: Gamma API market status + resolution field. Kalshi: settlement endpoint with explicit status. Deribit: get_delivery_prices for TWAP settlement value.
- Two-stage check: Query authoritative API for resolution status, then sanity-check outcome token prices against declared outcome. If mismatch, mark as `resolution_anomaly`, do not auto-settle.
- ResolutionResult enum: NotYetResolved, Resolved, Disputed, Ambiguous.
- WS price collapse is trigger for aggressive polling start, not settlement determination.

**Settlement Outcome Type:**
- Rich SettlementOutcome struct: event_id, venue, outcome (Yes/No/Ambiguous/Timeout), settlement_price (Option<Decimal>), resolved_at vs detected_at, resolution_source enum, raw_response.
- No numeric confidence field. Trust from ResolutionSource enum.

**Cross-Venue Divergence:**
- Per-leg settlement, not per-event. Each venue leg settles independently.
- SettlementDivergence annotation when venues disagree: divergence_type, basis_risk_score_at_entry, actual_impact_bps.
- Three analytics buckets for Phase 17: concordant, divergent, ambiguous.

**Auto-Settlement & P&L:**
- Settle each leg immediately as SettlementOutcome arrives.
- Both raw and fee-adjusted P&L per leg via SettledLeg struct: raw_pnl, entry_fee, exit_fee, slippage_estimate, net_pnl, fee_model_version.
- Position-level rollup: total raw P&L, total net P&L, total fees, total slippage, net-to-gross ratio.
- Daily rollup headline number = net P&L (fee-adjusted).

**Position Lifecycle & Memory Management:**
- Position states: Open -> PartiallySettled -> FullySettled -> evicted.
- Remove settled positions from active tracker after divergence annotation + JSONL flush.
- 48-hour retention in bounded `recently_settled` VecDeque (cap 100 or 48h).
- Timeout positions evicted immediately.
- Prometheus metrics at settlement: paper_trades_settled_total, paper_trade_net_pnl histogram, paper_trade_settlement_latency_seconds, paper_trade_divergence_total.
- JSONL SettlementRecord is single source of truth for historical analysis.

**Offline Backfill:**
- Checkpoint-anchored with configurable 7-day cap. On startup, scan only events with open positions at last checkpoint.
- Stale position handling: If time > max_lookback (7d), mark as resolution_timeout immediately.
- Backfill queue processed oldest-first using try_acquire on shared rate limiter.
- Missing checkpoint = clean start.
- Extend Phase 15 CheckpointState with settlement-related fields.

### Claude's Discretion

- Exact REST client implementation for each venue's settlement API
- Retry and backoff timing constants within the tiered cadence framework
- Internal data structures for the backfill queue and polling scheduler
- SettlementMonitor's internal state machine for tracking polling tier transitions

### Deferred Ideas (OUT OF SCOPE)

- Priority tiers for rate limiter (execution > settlement > feeds) -- v2 concern
- Automatic basis risk score updates from divergence data -- v2
- Dashboard UI for settlement tracking -- v2 MON-01
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| STTL-01 | System polls Deribit REST API for delivery/settlement prices after options expiry | Deribit `public/get_delivery_prices` endpoint verified: params(index_name, count, offset), returns `{data: [{date, delivery_price}]}`. Public, no auth. Match by expiry date. |
| STTL-02 | System polls Kalshi REST API for event resolution results | Kalshi `GET /markets/{ticker}` verified: returns `status` (determined/finalized), `result` (yes/no/scalar), `settlement_value_dollars`, `settlement_ts`. Requires RSA-PSS auth (existing pattern). |
| STTL-03 | System infers Polymarket event resolution from Gamma API (closed flag + price lock to 0 or 1) | Gamma API `GET /markets?id={condition_id}` verified: returns `closed`, `active`, `outcomePrices`, `umaResolutionStatuses`. No auth required. Two-stage: closed=true + outcomePrices lock to [0,1] or [1,0]. |
| STTL-04 | Settlement outcomes are normalized to a unified SettlementOutcome type across all venues | SettlementOutcome struct design: OutcomeKind enum (Yes/No/Ambiguous/Timeout), ResolutionSource enum, optional settlement_price (Decimal), resolved_at/detected_at timestamps. |
| STTL-05 | Settlement outcomes are logged to JSONL for historical analysis | SettlementRecord struct logged via daily-rotating JSONL (same pattern as TradeLogger in paper_trade/tracker.rs). Contains complete SettledLeg structs, divergence annotations, fee model versions. |
| STTL-06 | Paper trade positions are auto-settled when settlement outcomes arrive | SettlementOutcome sent via mpsc channel to PaperTradeTracker. Per-leg settlement: compute raw_pnl + fee-adjusted net_pnl using SpreadEngine's fee models. Position transitions Open -> PartiallySettled -> FullySettled. |
| STTL-07 | System detects and processes events that expired while offline (backfill on startup) | On startup: load checkpoint -> identify open positions with stale last_settlement_check -> enqueue for oldest-first backfill using try_acquire on shared rate limiter. 7-day cap -> resolution_timeout. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| reqwest | 0.12 | HTTP client for venue REST APIs | Already in Cargo.toml, used by discovery module |
| serde + serde_json | 1.0 | Serialization of settlement types and JSONL logging | Already in Cargo.toml, ubiquitous in codebase |
| rust_decimal | 1.40 | Settlement prices and P&L computation | Already in Cargo.toml, all financial values use Decimal |
| chrono | 0.4 | Timestamps (resolved_at, detected_at) and date-based JSONL rotation | Already in Cargo.toml |
| tokio | 1 | Async runtime, mpsc channels, interval timers | Already in Cargo.toml |
| governor | 0.8 | Rate limiting for settlement API calls | Already in Cargo.toml, shared VenueRateLimiter |
| metrics | 0.24 | Prometheus counters/gauges/histograms | Already in Cargo.toml |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| rsa + sha2 + base64 | 0.9/0.10/0.22 | Kalshi RSA-PSS authentication for settlement API | Only for Kalshi settlement polling (same auth as discovery) |
| tokio-util | 0.7 | CancellationToken for graceful shutdown | All background tasks use this pattern |
| anyhow + thiserror | 1.0/2.0 | Error handling | Settlement-specific error types via thiserror |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| reqwest (existing) | Separate HTTP client per venue | Would duplicate connection pools; existing shared client is correct |
| Manual backoff | backoff crate (existing) | Backoff crate is available but the four-tier cadence is custom; manual Duration management is simpler and matches the user's tier model exactly |
| New JSONL crate | Manual serde_json + BufWriter | Matches existing TradeLogger pattern; no new dependency needed |

**Installation:** No new crate dependencies. Zero additions to `Cargo.toml`. All libraries are already present.

## Architecture Patterns

### Recommended Project Structure
```
src/
├── settlement/              # NEW module
│   ├── mod.rs               # Module root, re-exports
│   ├── monitor.rs           # SettlementMonitor task (follows AlertMonitor pattern)
│   ├── types.rs             # SettlementOutcome, ResolutionResult, SettledLeg, SettlementRecord, etc.
│   ├── traits.rs            # ResolutionChecker trait
│   ├── deribit.rs           # DeribitResolutionChecker implementation
│   ├── kalshi.rs            # KalshiResolutionChecker implementation
│   ├── polymarket.rs        # PolymarketResolutionChecker implementation
│   └── config.rs            # SettlementConfig (TOML deserialization)
├── paper_trade/
│   ├── tracker.rs           # MODIFIED: add settlement_rx channel arm, per-leg settlement logic
│   ├── position.rs          # MODIFIED: add PartiallySettled status, per-leg settlement fields
│   └── aggregator.rs        # MODIFIED: accept net P&L for daily rollups
├── persistence/
│   └── checkpoint.rs        # MODIFIED: extend CheckpointState with settlement tracking
└── config/
    └── system.rs            # MODIFIED: add SettlementConfig to SystemConfig
```

### Pattern 1: SettlementMonitor Task (follows AlertMonitor pattern from Phase 14)
**What:** A long-running tokio task that periodically polls venue APIs to detect settlement outcomes.
**When to use:** This is the core pattern for the phase -- one task manages all venue polling.
**Example:**
```rust
// Follows AlertMonitor::run() pattern exactly
pub struct SettlementMonitor {
    registry: Arc<RwLock<EventRegistry>>,
    checkers: Vec<Box<dyn ResolutionChecker>>,
    tracked_events: HashMap<String, TrackedEvent>,
    settlement_tx: mpsc::Sender<SettlementOutcome>,
    liveness: Arc<PipelineLiveness>,
    config: SettlementConfig,
    cancel: CancellationToken,
}

impl SettlementMonitor {
    pub async fn run(mut self) {
        let mut interval = tokio::time::interval(
            Duration::from_secs(self.config.base_poll_interval_secs)
        );
        interval.tick().await; // skip first immediate tick

        loop {
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => {
                    tracing::info!("SettlementMonitor shutting down");
                    break;
                }
                _ = interval.tick() => {
                    self.poll_cycle().await;
                    self.liveness.record_settlement_check();
                }
            }
        }
    }
}
```

### Pattern 2: Trait-Based Resolution Checking
**What:** Each venue implements the same `ResolutionChecker` trait, enabling testability via mock implementations.
**When to use:** All venue-specific API logic is behind this trait boundary.
**Example:**
```rust
#[async_trait]
pub trait ResolutionChecker: Send + Sync {
    /// Check the resolution status of an event on this venue.
    async fn check_resolution(
        &self,
        event_id: &str,
        venue_instrument: &str,
    ) -> anyhow::Result<ResolutionResult>;

    /// Which venue this checker handles.
    fn venue(&self) -> Venue;
}

pub enum ResolutionResult {
    NotYetResolved,
    Resolved {
        outcome: OutcomeKind,
        settlement_price: Option<Decimal>,
        resolved_at: DateTime<Utc>,
    },
    Disputed {
        dispute_started: DateTime<Utc>,
    },
    Ambiguous {
        raw_data: String,
    },
}
```

### Pattern 3: Per-Leg Settlement with SettledLeg
**What:** Each venue leg of a paper trade position is settled independently as its SettlementOutcome arrives.
**When to use:** Position has legs on multiple venues (Deribit + Polymarket, Polymarket + Kalshi, etc.).
**Example:**
```rust
pub struct SettledLeg {
    pub venue: Venue,
    pub outcome: OutcomeKind,
    pub raw_pnl: Decimal,         // (settlement_price - entry_price) * notional * direction
    pub entry_fee: Decimal,        // From SpreadEngine's fee model at entry time
    pub exit_fee: Decimal,         // Exit fee (settlement is free for most venues)
    pub slippage_estimate: Decimal, // From entry adverse selection
    pub net_pnl: Decimal,          // raw_pnl - fees - slippage
    pub fee_model_version: String,  // Enable retroactive P&L recalc
    pub resolved_at: DateTime<Utc>,
    pub detected_at: DateTime<Utc>,
    pub resolution_source: ResolutionSource,
}
```

### Pattern 4: Four-Tier Polling Cadence State Machine
**What:** Each tracked event transitions through polling tiers based on time since trigger.
**When to use:** After the polling trigger fires for an event, controls how aggressively to poll.
**Example:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PollingTier {
    /// Not yet triggered for polling (before expiry).
    Waiting,
    /// 0-4 hours post-trigger: every 2-5 minutes.
    Aggressive { started_at: DateTime<Utc> },
    /// 4-96 hours: every 15-30 minutes (UMA DVM dispute window).
    Patient { started_at: DateTime<Utc> },
    /// 96h-7d: every 2-4 hours, WARN level logging.
    Lazy { started_at: DateTime<Utc> },
    /// Past 7 days: stop polling, emit resolution_timeout.
    TimedOut,
    /// Resolved -- no more polling needed.
    Resolved,
}
```

### Anti-Patterns to Avoid
- **Polling all venues on the same timer:** Each venue has different expected resolution times. Use per-event tier tracking, not a global sweep.
- **Using WS price as settlement determination:** Price collapse (0.00/1.00 lock) is a _trigger_ for when to start aggressive polling, NOT the authoritative settlement signal. Always verify with the venue's authoritative status field.
- **Waiting for all legs to settle before computing P&L:** Settle each leg immediately. Per-leg settlement preserves the true cross-venue edge. Waiting loses information.
- **Modifying the PaperTradeTracker's run loop parameters:** Add a new `settlement_rx` channel arm to the existing `select!` block. Do not alter the signal_rx/snapshot_rx handling.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HTTP REST calls with auth | Custom HTTP stack | `reqwest::Client` (existing) + Kalshi auth from `feed::kalshi::auth` | Connection pooling, TLS, redirect handling, timeout |
| Rate limiting | Custom token bucket | `governor::RateLimiter` via `VenueRateLimiter` (existing) | Shared rate budget across feeds + settlement + discovery |
| Atomic file writes | Manual temp+rename | `persistence::atomic::atomic_write` (existing) | Windows rename-over-existing fallback already implemented |
| JSONL daily rotation | Custom file rotation | Copy `TradeLogger` pattern from `paper_trade/tracker.rs` | Tested pattern with buffered writes and date-based rotation |
| Kalshi RSA-PSS signing | Custom crypto | `feed::kalshi::auth::sign_kalshi_request` (existing) | Correct RSA-PSS + SHA256 + Base64 implementation |
| Prometheus metrics | Custom reporting | `metrics::counter!`/`gauge!`/`histogram!` macros (existing) | Integrated with metrics-exporter-prometheus |

**Key insight:** Phase 16 introduces zero new dependencies. Every infrastructure component (HTTP client, rate limiter, auth, file I/O, metrics) already exists in the codebase from prior phases. The only new code is the settlement-specific business logic.

## Common Pitfalls

### Pitfall 1: Polymarket Has No Explicit "Resolved" Field
**What goes wrong:** Developer looks for a `resolved: true` field in the Gamma API response and concludes resolution detection is impossible.
**Why it happens:** Polymarket's Gamma API uses a combination of `closed: true` AND `outcomePrices` locking to [0,1] or [1,0] to indicate resolution. There is no single explicit "resolved" field.
**How to avoid:** Two-stage check: (1) `closed == true` AND (2) one outcome price >= 0.95 and the other <= 0.05 (with configurable threshold). The `umaResolutionStatuses` field may provide additional insight for disputed markets. Log `raw_response` for all resolution detections during paper trading for validation.
**Warning signs:** Tests that mock Polymarket responses with a `resolved` field that doesn't exist in the real API.

### Pitfall 2: Deribit Delivery Price Keyed by Index, Not Instrument
**What goes wrong:** Developer tries to look up delivery price by instrument name (e.g., "BTC-27JUN25-100000-C") and gets no results.
**Why it happens:** Deribit's `public/get_delivery_prices` endpoint is keyed by `index_name` (e.g., "btc_usd"), not by instrument. It returns a list of `{date, delivery_price}` pairs. You must match the expiry date of your instrument against the `date` field in the response.
**How to avoid:** Parse the instrument's expiry date from the `EventMapping.expiry` field, query `get_delivery_prices` with the correct `index_name`, and match the `date` field. The delivery price is the TWAP settlement value that determines binary outcome (was BTC above/below strike at settlement?).
**Warning signs:** Code that passes instrument names to the delivery price endpoint.

### Pitfall 3: Kalshi Status Lifecycle is Multi-Stage
**What goes wrong:** Developer checks for `status == "settled"` and misses markets in "determined" or "finalized" states.
**Why it happens:** Kalshi market lifecycle: initialized -> inactive -> active -> closed -> determined -> disputed -> amended -> finalized. The `result` and `settlement_value_dollars` fields are populated at "determined" stage, NOT "settled" stage. The `GET /markets?status=settled` filter exists but internally maps to determined+finalized.
**How to avoid:** Check for `status` in `["determined", "finalized"]` and verify `result` is non-empty. Use `settlement_value_dollars` for the actual settlement price. The `settlement_ts` field was added December 2025.
**Warning signs:** Tests that only check for a single terminal status string.

### Pitfall 4: Rate Limiter Contention During Backfill
**What goes wrong:** Settlement backfill on startup consumes the entire API rate budget, starving live feed connections.
**Why it happens:** Backfill queue tries to catch up on missed settlements and fires rapid requests without yielding.
**How to avoid:** Use `try_acquire` (non-blocking) on the shared rate limiter for backfill requests. If the limiter denies, skip to next tick. Live feeds take priority during startup contention. Process backfill queue oldest-first with deliberate yielding.
**Warning signs:** Feed disconnections during startup when there are stale positions to backfill.

### Pitfall 5: Checkpoint State Version Mismatch After Adding Settlement Fields
**What goes wrong:** Adding settlement fields to CheckpointState breaks deserialization of existing v1 checkpoints.
**Why it happens:** New required fields without defaults cause `serde_json::from_str` to fail on old checkpoints.
**How to avoid:** Use `#[serde(default)]` on all new fields added to CheckpointState. Bump `version` to 2 but accept version 1 with default values for new fields. The existing recovery code already handles missing files gracefully; make it also handle schema evolution gracefully.
**Warning signs:** Startup crash when a v1 checkpoint exists and the code expects v2 fields.

### Pitfall 6: Kalshi Rule 6.3(c) Ambiguous Resolution
**What goes wrong:** System treats all Kalshi settlements as clean binary (YES/NO) and panics or produces incorrect P&L for ambiguous resolutions.
**Why it happens:** Under Rule 6.3(c), if an event doesn't resolve cleanly, Kalshi settles at the last-traded price (not binary 0/1). The `result` field will be "scalar" and `settlement_value_dollars` will be a fractional value.
**How to avoid:** Explicit `OutcomeKind::Ambiguous` handling. When Kalshi `result == "scalar"` or `settlement_value_dollars` is not 0.00 or 1.00, mark as Ambiguous with the actual settlement value. Quarantine from signal quality metrics (Phase 17).
**Warning signs:** P&L computation that assumes settlement_price is always 0 or 1.

### Pitfall 7: Windows Rename Atomicity
**What goes wrong:** Settlement JSONL files get corrupted on Windows when writing and the process crashes mid-write.
**Why it happens:** Windows `rename()` is not atomic when the target exists. The existing `atomic_write` utility handles this, but new JSONL writers might not use it.
**How to avoid:** Use `BufWriter` with periodic `flush()` for JSONL append operations (same as existing TradeLogger). For checkpoint writes, always use `persistence::atomic::atomic_write`. Never write the checkpoint file directly.
**Warning signs:** Settlement JSONL files with truncated last lines after a crash.

## Code Examples

### Deribit Settlement Resolution Check
```rust
// Source: Deribit public API (https://docs.deribit.com)
// GET /api/v2/public/get_delivery_prices?index_name=btc_usd&count=10

#[derive(Debug, Deserialize)]
struct DeribitDeliveryResponse {
    result: DeribitDeliveryResult,
}

#[derive(Debug, Deserialize)]
struct DeribitDeliveryResult {
    records_total: u64,
    data: Vec<DeribitDeliveryEntry>,
}

#[derive(Debug, Deserialize)]
struct DeribitDeliveryEntry {
    date: String,           // "2025-06-27"
    delivery_price: f64,    // 97234.56 (TWAP settlement value)
}

impl DeribitResolutionChecker {
    async fn check_resolution(
        &self,
        event_id: &str,
        instrument: &str,
        expiry_date: &str,  // "2025-06-27"
        strike: Decimal,
        direction: &Direction,
    ) -> anyhow::Result<ResolutionResult> {
        let index_name = "btc_usd"; // derived from asset
        let url = format!(
            "{}/api/v2/public/get_delivery_prices",
            self.base_url
        );
        let resp: DeribitDeliveryResponse = self.client
            .get(&url)
            .query(&[("index_name", index_name), ("count", "30")])
            .send()
            .await?
            .json()
            .await?;

        // Find delivery price matching the expiry date
        for entry in &resp.result.data {
            if entry.date == expiry_date {
                let delivery = Decimal::from_f64_retain(entry.delivery_price)
                    .unwrap_or_default();
                let outcome = match direction {
                    Direction::Above => {
                        if delivery >= strike { OutcomeKind::Yes } else { OutcomeKind::No }
                    }
                    Direction::Below => {
                        if delivery <= strike { OutcomeKind::Yes } else { OutcomeKind::No }
                    }
                };
                return Ok(ResolutionResult::Resolved {
                    outcome,
                    settlement_price: Some(delivery),
                    resolved_at: Utc::now(), // Deribit doesn't give exact resolution time
                });
            }
        }
        Ok(ResolutionResult::NotYetResolved)
    }
}
```

### Kalshi Settlement Resolution Check
```rust
// Source: Kalshi API (https://docs.kalshi.com/api-reference/market/get-market)
// GET /trade-api/v2/markets/{ticker}

#[derive(Debug, Deserialize)]
struct KalshiMarketResponse {
    market: KalshiMarketDetail,
}

#[derive(Debug, Deserialize)]
struct KalshiMarketDetail {
    ticker: String,
    status: String,              // "determined", "finalized", etc.
    result: Option<String>,      // "yes", "no", "scalar", or ""
    settlement_value_dollars: Option<String>, // FixedPointDollars
    settlement_ts: Option<String>,  // ISO8601 datetime
}

impl KalshiResolutionChecker {
    async fn check_resolution(
        &self,
        ticker: &str,
    ) -> anyhow::Result<ResolutionResult> {
        // Sign request with RSA-PSS (reuse existing auth)
        let path = format!("/trade-api/v2/markets/{}", ticker);
        let timestamp_ms = Utc::now().timestamp_millis();
        let signature = sign_kalshi_request(
            &self.private_key, timestamp_ms, "GET", &path
        )?;

        let resp: KalshiMarketResponse = self.client
            .get(format!("{}{}", self.rest_url, path))
            .header("KALSHI-ACCESS-KEY", &self.api_key_id)
            .header("KALSHI-ACCESS-SIGNATURE", &signature)
            .header("KALSHI-ACCESS-TIMESTAMP", timestamp_ms.to_string())
            .send()
            .await?
            .json()
            .await?;

        let market = &resp.market;
        match market.status.as_str() {
            "determined" | "finalized" => {
                match market.result.as_deref() {
                    Some("yes") => Ok(ResolutionResult::Resolved {
                        outcome: OutcomeKind::Yes,
                        settlement_price: parse_settlement_value(&market.settlement_value_dollars),
                        resolved_at: parse_settlement_ts(&market.settlement_ts),
                    }),
                    Some("no") => Ok(ResolutionResult::Resolved {
                        outcome: OutcomeKind::No,
                        settlement_price: parse_settlement_value(&market.settlement_value_dollars),
                        resolved_at: parse_settlement_ts(&market.settlement_ts),
                    }),
                    Some("scalar") => Ok(ResolutionResult::Ambiguous {
                        raw_data: serde_json::to_string(&market)
                            .unwrap_or_default(),
                    }),
                    _ => Ok(ResolutionResult::NotYetResolved),
                }
            }
            "disputed" => Ok(ResolutionResult::Disputed {
                dispute_started: Utc::now(),
            }),
            _ => Ok(ResolutionResult::NotYetResolved),
        }
    }
}
```

### Polymarket Resolution Check
```rust
// Source: Polymarket Gamma API (https://gamma-api.polymarket.com)
// GET /markets?id={condition_id}

#[derive(Debug, Deserialize)]
struct GammaMarketResponse {
    #[serde(rename = "conditionId")]
    condition_id: String,
    active: bool,
    closed: bool,
    outcomes: Option<String>,         // JSON string: ["Yes", "No"]
    #[serde(rename = "outcomePrices")]
    outcome_prices: Option<String>,   // JSON string: ["0.95", "0.05"]
    #[serde(rename = "umaResolutionStatuses")]
    uma_resolution_statuses: Option<String>,
}

impl PolymarketResolutionChecker {
    async fn check_resolution(
        &self,
        condition_id: &str,
    ) -> anyhow::Result<ResolutionResult> {
        let resp: Vec<GammaMarketResponse> = self.client
            .get(format!("{}/markets", self.gamma_api_url))
            .query(&[("id", condition_id)])
            .send()
            .await?
            .json()
            .await?;

        let market = resp.first()
            .ok_or_else(|| anyhow::anyhow!("market not found"))?;

        if !market.closed {
            return Ok(ResolutionResult::NotYetResolved);
        }

        // Parse outcome prices from JSON string
        let prices: Vec<f64> = market.outcome_prices
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        // Two-stage check: closed=true AND price lock
        if prices.len() >= 2 {
            let yes_price = prices[0];
            let no_price = prices[1];
            let threshold = 0.95; // configurable

            if yes_price >= threshold && no_price <= (1.0 - threshold) {
                return Ok(ResolutionResult::Resolved {
                    outcome: OutcomeKind::Yes,
                    settlement_price: None, // Binary, no continuous price
                    resolved_at: Utc::now(),
                });
            }
            if no_price >= threshold && yes_price <= (1.0 - threshold) {
                return Ok(ResolutionResult::Resolved {
                    outcome: OutcomeKind::No,
                    settlement_price: None,
                    resolved_at: Utc::now(),
                });
            }
        }

        // Check for UMA dispute
        // umaResolutionStatuses might indicate ongoing dispute
        if let Some(ref uma_status) = market.uma_resolution_statuses {
            if uma_status.contains("disputed") || uma_status.contains("DVM") {
                return Ok(ResolutionResult::Disputed {
                    dispute_started: Utc::now(),
                });
            }
        }

        // Closed but prices not yet locked -- keep polling
        Ok(ResolutionResult::NotYetResolved)
    }
}
```

### SettlementOutcome Channel Integration with PaperTradeTracker
```rust
// Add new channel arm to PaperTradeTracker::run() select! block
// Source: Follows existing signal_rx/snapshot_rx pattern in paper_trade/tracker.rs

// In PaperTradeTracker::run():
settlement = settlement_rx.recv() => {
    match settlement {
        Some(outcome) => {
            self.handle_settlement(outcome);
        }
        None => {
            tracing::info!("settlement channel closed");
        }
    }
}

fn handle_settlement(&mut self, outcome: SettlementOutcome) {
    // Find open positions for this event+venue
    for pos in &mut self.open {
        if pos.event_id == outcome.event_id {
            // Compute per-leg settlement
            let leg = compute_settled_leg(&outcome, pos);
            // Record in position
            pos.record_settled_leg(leg);
            // If all legs settled, transition to FullySettled
            if pos.all_legs_settled() {
                pos.finalize_settlement();
                self.aggregator.record_trade(pos);
                // Log SettlementRecord to JSONL
                self.log_settlement_record(pos, &outcome);
            }
        }
    }
    // Evict fully settled positions
    self.evict_settled_positions();
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Kalshi integer strike fields | Kalshi `floor_strike`/`cap_strike` dollar fields | Feb 2026 | Already handled in existing discovery code |
| Kalshi no settlement_ts | `settlement_ts` field on GET /markets | Dec 2025 | New field available for settlement timestamp |
| Kalshi no settlement_value_dollars on REST | `settlement_value_dollars` on GET /markets | API changelog 2025 | Direct access to settlement price without portfolio endpoint |
| Kalshi no WS settlement value | `market_lifecycle_v2` WS with `settlement_value` | Feb 26 2026 | Future: could use WS push instead of REST polling (v2 optimization) |

**Deprecated/outdated:**
- Kalshi `settlement_value` (integer cents): Replaced by `settlement_value_dollars` (FixedPointDollars string). Use the `_dollars` variant.
- Kalshi `GET /portfolio/settlements`: This is per-portfolio, not per-market. Use `GET /markets/{ticker}` for market-level settlement data.

## Open Questions

1. **Polymarket `umaResolutionStatuses` field structure**
   - What we know: Field exists in Gamma API response, relates to UMA oracle dispute status
   - What's unclear: Exact JSON structure (string? array? object?), possible values, and how it maps to dispute stages (optimistic, DVM vote)
   - Recommendation: Parse as `Option<String>`, log raw value during paper trading to build understanding. Use `closed` + `outcomePrices` as primary resolution signal; `umaResolutionStatuses` as supplementary dispute indicator.

2. **Deribit delivery price timing precision**
   - What we know: Delivery prices appear at 08:00 UTC on expiry day via TWAP calculation
   - What's unclear: Exact delay between 08:00 UTC and when `get_delivery_prices` reflects the new day's price
   - Recommendation: First poll after 08:00 UTC with retry on NotYetResolved. The four-tier cadence handles this naturally -- aggressive polling for first 4 hours after trigger.

3. **Polymarket Gamma API rate limits**
   - What we know: The existing discovery module already polls Gamma API without issues
   - What's unclear: Official rate limit documentation for Gamma API is sparse
   - Recommendation: Use existing `polymarket.rate_limit_per_second` config (default 10 req/s) via shared rate limiter. Settlement polling adds minimal additional load (one request per active event per poll cycle).

4. **Existing PaperPosition lacks per-leg structure**
   - What we know: Current `PaperPosition` tracks a single event-level position, not per-venue legs
   - What's unclear: Whether to refactor PaperPosition to have explicit legs or add leg tracking alongside
   - Recommendation: Add a `settled_legs: Vec<SettledLeg>` field to PaperPosition. The entry is still event-level (both legs entered together), but settlement tracks per-leg outcomes. This minimizes refactoring of the existing position lifecycle while adding the per-leg settlement capability.

## Sources

### Primary (HIGH confidence)
- Deribit API `public/get_delivery_prices` - Verified via official docs (https://docs.deribit.com). Returns `{data: [{date, delivery_price}]}` keyed by `index_name`. Public endpoint, no auth.
- Kalshi API `GET /markets/{ticker}` - Verified via official docs (https://docs.kalshi.com/api-reference/market/get-market). Returns `status`, `result`, `settlement_value_dollars`, `settlement_ts`. RSA-PSS auth required.
- Polymarket Gamma API `GET /markets` - Verified via live API response (https://gamma-api.polymarket.com/markets). Returns `closed`, `active`, `outcomePrices`, `umaResolutionStatuses`. No auth required.
- Kalshi API changelog (https://docs.kalshi.com/changelog) - `settlement_ts` added Dec 2025, `settlement_value_dollars` available on GET /markets.

### Secondary (MEDIUM confidence)
- Polymarket UMA resolution process - Verified from Polymarket docs (https://docs.polymarket.com/developers/resolution/UMA) + UMA documentation. Optimistic oracle 2h liveness, DVM dispute 48-96h.
- Kalshi market status lifecycle: initialized -> inactive -> active -> closed -> determined -> disputed -> amended -> finalized. Verified from multiple Kalshi API doc pages.

### Tertiary (LOW confidence)
- Polymarket `umaResolutionStatuses` field structure - Only observed as a field in live API responses, exact value space undocumented. Needs runtime validation during paper trading.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All libraries already in Cargo.toml, zero new dependencies
- Architecture: HIGH - Follows exact AlertMonitor pattern from Phase 14 (code verified), trait-based venue abstraction matches discovery module pattern
- Venue APIs: MEDIUM-HIGH - Deribit and Kalshi endpoints verified via official docs. Polymarket Gamma API fields verified via live response, but `umaResolutionStatuses` semantics are LOW confidence.
- Pitfalls: HIGH - Based on actual API response analysis and existing codebase patterns
- Settlement P&L computation: MEDIUM - Per-leg computation logic is straightforward, but integration with existing PaperPosition requires careful field additions (settled_legs, PartiallySettled status)

**Research date:** 2026-02-24
**Valid until:** 2026-03-24 (stable APIs, 30-day validity)
