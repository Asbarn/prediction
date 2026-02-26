# Feature Landscape: v1.2 Automated Event Management

**Domain:** Market discovery, cross-venue event matching, event lifecycle management for cross-venue prediction market arbitrage
**Researched:** 2026-02-26
**Confidence:** HIGH (discovery APIs, lifecycle management) / MEDIUM (Polymarket structured matching, expiry date alignment heuristics)

**Scope note:** This research covers ONLY the new features for v1.2 Automated Event Management. Existing v1.0/v1.1 features (feeds, pricing, spread engines, paper trading, settlement tracking, signal analysis, persistence, alerting) are already built. The v1.2 milestone builds directly on: `EventRegistry`, `ContractLifecycleManager`, `ConfigReloader`, `events.toml`, and the existing `events::discovery` module which already has scaffolded discovery functions for all three venues.

**Critical existing code inventory:** The codebase already has substantial v1.2 infrastructure:
- `discover_deribit()` -- polls `/api/v2/public/get_instruments`, returns `Vec<DiscoveredInstrument>` with structured fields (no auth required)
- `discover_kalshi()` -- polls `/trade-api/v2/markets` with RSA-PSS auth, pagination via cursor, parses `floor_strike`/`cap_strike`
- `discover_polymarket()` -- polls Gamma API `/markets`, returns `Vec<PolymarketMarketInfo>` (deactivation monitoring only in current form)
- `find_cross_venue_candidates()` -- exact four-field matching (asset + strike + expiry + direction)
- `filter_new_candidates()` -- deduplicates against existing registry
- `flag_novel_instruments()` -- flags unmatched single-venue instruments
- `append_candidate_to_toml()` / `mark_expired_in_toml()` -- toml_edit-based atomic writers preserving formatting
- `ContractLifecycleManager::poll_cycle()` -- full orchestration loop with per-venue interval tracking, candidate appending, expiry detection, Deribit roll handling, expiry warnings, risk cache population, and registry refresh
- `ConfigReloader` -- file-system watcher with 500ms debounce, distributes new `AppConfig` via `watch` channel
- `DiscoveryConfig` -- per-venue poll intervals, currency/series filters in events.toml
- `LifecycleStatus` enum: Active, Expiring, Expired

The question is: what features are MISSING from this already-built infrastructure?

---

## Table Stakes

Features the automated event management system must have. Without these, the operator still needs to manually curate events.toml -- the entire goal of v1.2 is defeated.

### TS-1: Polymarket Structured Market Discovery

The current `discover_polymarket()` implementation fetches markets but is limited to deactivation monitoring. It does NOT extract structured fields (asset, strike, direction, expiry) from Polymarket market data, making it impossible to include Polymarket in cross-venue candidate matching. This is the single largest gap.

| Feature | Why Expected | Complexity | Dependencies on Existing Code |
|---------|--------------|------------|-------------------------------|
| Polymarket crypto market filtering | Current discovery fetches all active markets. Must filter to crypto/BTC price-level markets using Gamma API `tag_id=21` (Crypto category) or category-based filtering to avoid processing thousands of irrelevant political/sports markets. | LOW | Existing `discover_polymarket()` fetches from `/markets`. Add query parameter `tag_id=21` or use `/events?tag_id=21` endpoint. |
| Extract structured fields from Polymarket group events | Polymarket structures BTC price markets as grouped events (e.g., "What price will Bitcoin hit in February?") with sub-markets per price level. Each sub-market has `groupItemTitle` like "up 150,000" containing the strike price. Must parse `groupItemTitle` and parent event metadata to extract asset, strike, direction. | HIGH | Existing `PolymarketMarketInfo` struct needs expansion: add `groupItemTitle`, `groupItemThreshold`, parent event fields. New: regex/pattern parser for `groupItemTitle` (e.g., "up 150,000" -> direction=Above, strike=150000). |
| Polymarket expiry date extraction | Polymarket provides `endDateIso` on markets and `endDate` on parent events. Must normalize these to `NaiveDate` for cross-venue matching. Polymarket end dates often differ from Deribit/Kalshi expiries (monthly vs specific Friday). | MEDIUM | Existing `PolymarketMarketInfo` has `end_date_iso: Option<String>`. New: parse to `NaiveDate`, apply expiry alignment tolerance for cross-venue matching. |
| Polymarket token ID extraction for mapping | When proposing a candidate match, must capture `conditionId` and `clobTokenIds` (Yes token) for the events.toml `PolymarketMapping`. The existing `PolymarketMapping` struct requires both `condition_id` and `token_id`. | LOW | Existing `PolymarketMarketInfo` has `condition_id` field. Need `tokens` array parsing (existing struct has `Vec<PolymarketToken>`). Map "Yes" outcome token_id for the mapping. |

**Polymarket data structure reality (from API investigation):**

Polymarket does NOT provide machine-readable structured fields for asset, strike price, or direction. Price thresholds exist only in narrative form within the `question` text (e.g., "Will Bitcoin reach $150,000 in February?") and the `groupItemTitle` display label (e.g., "up 150,000"). This means:

1. `groupItemTitle` is the most reliable semi-structured source -- consistent format for price-level markets within a group
2. The `question` text is free-form and varies by market creator
3. There is no `strike`, `asset`, or `direction` field in the API response

**Parsing approach:** Use `groupItemTitle` regex patterns (e.g., `(up|down)\s+([\d,]+)`) as the primary extraction method for grouped price markets. Fall back to `question` text regex for standalone markets. This is inherently fragile -- format changes will break extraction. Mitigation: log extraction failures, require human approval via `approved = false`.

### TS-2: Expiry Date Alignment and Tolerance Matching

The current `find_cross_venue_candidates()` requires exact four-field matching including exact expiry date. In practice, venues use different expiry dates for economically equivalent events:

- **Deribit**: Options expire on specific Fridays (e.g., 2025-06-27 at 08:00 UTC)
- **Kalshi**: Markets close at `close_time`, often end-of-day or end-of-month (e.g., 2025-06-30T23:59:59Z)
- **Polymarket**: End dates are typically end-of-month (e.g., 2026-03-01T05:00:00Z)

An exact match on expiry date will miss the majority of real cross-venue matches.

| Feature | Why Expected | Complexity | Dependencies on Existing Code |
|---------|--------------|------------|-------------------------------|
| Configurable expiry date tolerance window | Allow matches where expiry dates differ by up to N days (configurable, default 7). Events within the same economic window (e.g., "BTC above $100K by end of June") should match even if Deribit says June 27 and Kalshi says June 30. | MEDIUM | Existing `MatchKey` has exact `expiry: NaiveDate` field. New: tolerance comparison that groups expiries within a window. Must update `find_cross_venue_candidates()` to use fuzzy expiry matching while keeping asset/strike/direction exact. |
| Expiry confidence scoring | When expiries match exactly, confidence=HIGH. When they differ by 1-3 days, confidence=MEDIUM. When 4-7 days, confidence=LOW. Confidence is logged with the candidate proposal for operator review. | LOW | New field on `CandidateMapping` or in the proposal log. Computed from the actual day difference. |
| Settlement timing capture in proposals | Auto-populate `SettlementMetadata` fields (deribit_settlement_time, venue resolution times) in candidate proposals based on discovered close_time/expiration_time data. Reduces manual curation needed after approval. | MEDIUM | Existing `SettlementMetadata` struct in `EventMapping`. Existing Deribit `expiration_timestamp` and Kalshi `close_time` in discovered instruments. New: auto-fill settlement metadata during candidate generation. |

### TS-3: Event Retirement and Cleanup

The current `mark_expired_in_toml()` sets `status = "expired"` but expired entries accumulate forever. Over months of operation, events.toml will grow unboundedly with stale entries, degrading readability and parse performance.

| Feature | Why Expected | Complexity | Dependencies on Existing Code |
|---------|--------------|------------|-------------------------------|
| Expired event archival | Move entries with `status = "expired"` that are older than a configurable retention period (e.g., 30 days past expiry) from events.toml to an `events_archive.toml` or remove them entirely. Keep events.toml clean and focused on active/pending events. | MEDIUM | Existing `mark_expired_in_toml()` sets status. Existing `toml_edit`-based writers. New: retention period config, archive writer, periodic cleanup in lifecycle poll_cycle. |
| Unapproved candidate expiration | Auto-discovered candidates (`approved = false`) that sit unapproved past their expiry date should be auto-cleaned. No point keeping proposals for events that already happened. | LOW | Existing `approved` and `expiry` fields on `EventMapping`. New: check in poll_cycle -- if `!approved && expiry < today`, mark as expired or remove. |
| Retired status addition | Add a `Retired` status to `LifecycleStatus` for events that have been fully settled and archived. Distinguishes "recently expired, settlement pending" from "done, can be cleaned up." | LOW | Existing `LifecycleStatus` enum: Active, Expiring, Expired. New: add `Retired` variant. Transition: Expired -> Retired after settlement confirmed or retention period elapsed. |

### TS-4: Proposal Notification and Approval Workflow

The current system appends candidates with `approved = false` and logs an info message. The operator must manually find and edit events.toml to approve. This workflow needs to be clear and low-friction.

| Feature | Why Expected | Complexity | Dependencies on Existing Code |
|---------|--------------|------------|-------------------------------|
| Structured proposal log emission | When a candidate is discovered, emit a structured tracing log with all details: event_id, matched venues, instruments, expiry dates (per venue), confidence score, and what the operator needs to verify. Machine-parseable for monitoring. | LOW | Existing `tracing::info!` in `poll_cycle` logs event_id, deribit, kalshi. New: expand to include all matched fields, confidence, and a human-readable summary. |
| Prometheus metrics for pending proposals | Gauge metric `lifecycle_pending_proposals` showing count of `approved = false` entries. Counter `lifecycle_proposals_total` for total proposals made. Enables Alertmanager rules for "new proposals awaiting review." | LOW | Existing `metrics::counter!("lifecycle_candidates_discovered")`. New: add gauge for pending count, updated each poll cycle. |
| Approval triggers config reload | When operator sets `approved = true` in events.toml, the existing `ConfigReloader` file watcher detects the change and reloads. The lifecycle manager must then update the runtime `EventRegistry` and trigger subscription management for the newly approved mapping. | LOW | Existing `ConfigReloader` watches config directory. Existing `EventRegistry.refresh()`. The pipeline already reacts to registry changes. This may already work -- needs verification. |
| Approval validation | When a candidate is approved, validate that the mapping is complete (has at least 2 venue instruments), the instruments still exist/are active on their venues, and the expiry has not passed. Reject invalid approvals with clear error messages. | MEDIUM | Existing `config::validation` module. New: runtime validation on config reload that checks approved mappings against current venue state. |

### TS-5: Live Subscription Management

When a new event mapping is approved, the system must start receiving market data for the new instruments without a full restart.

| Feature | Why Expected | Complexity | Dependencies on Existing Code |
|---------|--------------|------------|-------------------------------|
| Dynamic feed subscription for new instruments | When a mapping transitions from unapproved to approved, subscribe to the relevant WebSocket channels for the new instruments on each venue. | HIGH | Existing venue WebSocket clients (`DeribitClient`, `KalshiClient`, `PolymarketClient`) manage subscriptions at startup. Need to expose `subscribe(instrument)` / `unsubscribe(instrument)` methods that can be called at runtime. Each venue has different subscription semantics. |
| Dynamic feed unsubscription for expired instruments | When a mapping transitions to expired/retired, unsubscribe from the venue WebSocket channels to free up connection resources and reduce noise. | MEDIUM | Depends on dynamic subscription capability above. New: unsubscribe on status transition. Must handle gracefully if the instrument is shared with another active mapping. |
| Config-change-driven subscription reconciliation | On config reload (file watcher or SIGHUP), compute the diff between old and new active instrument sets. Subscribe to new instruments, unsubscribe from removed instruments. | HIGH | Existing `ConfigReloader` distributes new `AppConfig` via `watch` channel. Existing `EventRegistry.refresh()` updates the in-memory registry. New: subscription reconciliation logic that compares old vs new `active_approved()` sets and issues subscribe/unsubscribe commands. |

---

## Differentiators

Features that go beyond basic automated discovery and provide smarter matching, richer context, or reduced operator burden. Not required for v1.2 MVP but significantly increase the system's autonomy.

### D-1: Smart Cross-Venue Matching Heuristics

| Feature | Value Proposition | Complexity | Dependencies on Existing Code |
|---------|-------------------|------------|-------------------------------|
| Partial venue matching (2 of 3 venues) | Current matching requires 2+ venues with matching instruments. But a Deribit option and a Kalshi market may match while no Polymarket equivalent exists. The system should propose partial matches and flag missing venues for the operator. | LOW | Existing `find_cross_venue_candidates()` already returns 2+ venue matches. This is already working. Enhancement: when a partial match (Deribit+Kalshi but no Polymarket) is proposed, explicitly note the missing venue in the proposal log. |
| Strike price normalization across venues | Kalshi uses `floor_strike` (dollars), Deribit uses `strike` (option strike), Polymarket embeds price in question text. Different rounding or representation (100000 vs 100,000 vs $100K) could prevent matches. Normalize all to `Decimal` before matching. | LOW | Existing code already normalizes to `Decimal` via `Decimal::from_f64_retain()`. Enhancement: strip currency symbols, commas, and "K"/"M" suffixes during Polymarket text parsing. |
| Temporal clustering of proposals | If the system discovers 50 new Deribit options at different strikes for the same expiry, it should batch proposals rather than emitting 50 individual log lines. Group proposals by expiry date and present as a summary. | LOW | New: accumulate proposals per poll cycle and emit a single summary log with counts by expiry. |

### D-2: Discovery Intelligence

| Feature | Value Proposition | Complexity | Dependencies on Existing Code |
|---------|-------------------|------------|-------------------------------|
| Venue instrument change detection | Track the full set of active instruments per venue across poll cycles. Detect not just new instruments but also deactivations, status changes, and metadata updates. Emit structured change logs. | MEDIUM | Existing `all_discovered` in poll_cycle is transient. New: persist the previous poll's instrument set (in memory) and diff against current. |
| Discovery health monitoring | Track discovery success/failure rates per venue. Alert if a venue consistently fails discovery (API down, auth expired, rate limited). Distinct from feed health -- discovery uses REST, not WebSocket. | LOW | Existing `metrics::counter!("lifecycle_discovery_polls")` per venue. New: add failure counter and success rate computation. Alert if failure rate exceeds threshold. |
| Polymarket question text pattern library | Maintain a configurable set of regex patterns for extracting structured data from Polymarket question text. Start with BTC price patterns, extensible to ETH and other assets. Log unmatched patterns for operator review and pattern expansion. | MEDIUM | New: pattern library in config (TOML array of regex patterns with named capture groups). Applied during Polymarket discovery. Fallback: log raw question text for manual review. |

### D-3: Candidate Quality Scoring

| Feature | Value Proposition | Complexity | Dependencies on Existing Code |
|---------|-------------------|------------|-------------------------------|
| Match confidence composite score | Combine expiry alignment confidence, venue count (2 vs 3), and liquidity indicators into a single match quality score. Higher scores = more likely to be profitable arb opportunities. | MEDIUM | New: composite scoring function. Inputs: expiry day difference, number of matched venues, instrument activity status. Output: 0.0-1.0 confidence. |
| Liquidity pre-screening | Before proposing a candidate, check if the discovered instruments have meaningful volume/open interest. Low-liquidity matches are real but untradeable. Flag low-liquidity candidates differently. | MEDIUM | Deribit `get_instruments` does not return volume (need separate `get_book_summary`). Kalshi markets response includes volume. Polymarket includes `volume`. New: optional liquidity check during discovery. |
| Historical match accuracy tracking | Track how often auto-proposed candidates were approved vs rejected by the operator. Over time, this reveals whether the matching criteria are too loose (many rejections) or too tight (missing real matches). | LOW | New: Prometheus counters for proposals_approved vs proposals_rejected. Compute approval rate periodically. |

---

## Anti-Features

Features that seem relevant to automated event management but should be explicitly avoided.

| Anti-Feature | Why It Seems Relevant | Why Avoid | What to Do Instead |
|--------------|----------------------|-----------|-------------------|
| NLP/ML-based Polymarket question parsing | "Use a language model to extract structured data from free-form questions" | Adds heavyweight dependencies (tokenizers, models, or API calls to external services). Polymarket crypto price markets follow a small number of predictable patterns -- regex is sufficient. ML is overkill for "Will Bitcoin reach $150,000?" and unreliable for edge cases. Also violates the zero-new-dependency principle established in v1.1. | Regex pattern library on `groupItemTitle` and `question` text. Log unmatched patterns for manual review. Keep patterns in TOML config for easy extension without recompilation. |
| Full-text fuzzy matching across all venues | "Use Levenshtein distance or TF-IDF to match market descriptions across venues" | Polymarket uses free-form English questions, Deribit uses structured instrument names, Kalshi uses structured tickers. These are fundamentally different representations. Fuzzy text matching would produce high false positive rates and miss real matches. | Exact structured field matching (asset + strike + direction) with fuzzy expiry tolerance. This is what the codebase already does for Deribit+Kalshi. Extend to Polymarket only after extracting structured fields from `groupItemTitle`. |
| Automatic approval of high-confidence matches | "If confidence > 0.95, auto-approve without human review" | For a solo trader managing real arbitrage positions (v2), every mapping directly affects capital allocation. A false match between venues with different settlement criteria could cause total loss on both legs. The approval gate is a critical safety mechanism. | Always write candidates with `approved = false`. Make approval easy (edit one field in TOML), not automatic. Log enough context that approval takes <30 seconds per candidate. |
| Real-time event stream subscription for discovery | "Subscribe to venue WebSocket feeds for new instrument notifications instead of polling" | Deribit does not offer a WebSocket notification for new instruments. Kalshi does not offer real-time market listing notifications. Polymarket has no push API for new event creation. All three venues require REST polling for discovery. New instruments appear on a daily/weekly cadence, not millisecond -- polling every 5-10 minutes is more than sufficient. | REST polling at configured intervals per venue, which is already implemented in `ContractLifecycleManager`. |
| Multi-asset discovery (ETH, SOL, etc.) in v1.2 | "While building discovery, add support for all assets" | The existing system is BTC-only by design decision (highest cross-venue liquidity). Adding multi-asset discovery before BTC event management is validated adds complexity without value. ETH options on Deribit have different strike intervals, Kalshi may not have ETH markets, and Polymarket ETH market question formats may differ. | Keep `deribit_currencies = ["BTC"]` and `kalshi_series_tickers = ["KXBTC"]` in discovery config. The architecture supports multi-asset (config-driven), but v1.2 validates the automation with BTC only. |
| Automated events.toml conflict resolution | "If two concurrent discovery cycles modify events.toml, merge changes" | Single-binary, single-writer architecture. The lifecycle manager is the only writer. ConfigReloader is read-only. There is no concurrent write scenario. Adding merge logic adds complexity for a problem that does not exist. | Keep single-writer pattern. Lifecycle manager owns all writes to events.toml. Operator edits (approval) happen between poll cycles and are read via ConfigReloader. |
| Database-backed event store replacing events.toml | "TOML files don't scale, use SQLite for event storage" | events.toml will contain at most dozens to low-hundreds of entries (BTC binary events across 3 venues with monthly/quarterly expiries). TOML is human-readable, git-trackable, and requires zero dependencies. A database adds operational complexity for a solo-trader system. | Keep events.toml as the source of truth. The `toml_edit`-based writers preserve formatting and comments. Archive old entries to keep the file manageable. |

---

## Feature Dependencies

```
[Polymarket Structured Discovery (TS-1)]
    |
    +--> Requires: Existing discover_polymarket() (fetches raw market data)
    +--> Requires: Gamma API tag_id filtering (crypto category)
    +--> Requires: groupItemTitle regex parser (new)
    +--> Enables: Three-venue cross-venue matching
    |
    v
[Expiry Date Alignment (TS-2)]
    |
    +--> Requires: Existing find_cross_venue_candidates() (exact matching)
    +--> Modifies: MatchKey expiry comparison (tolerance window)
    +--> Enables: Real-world cross-venue matching where dates differ
    |
    v
[Cross-Venue Candidate Matching] (combined TS-1 + TS-2)
    |
    +--> Feeds into: Existing append_candidate_to_toml()
    +--> Feeds into: Proposal Notification (TS-4)

[Event Retirement (TS-3)]
    |
    +--> Requires: Existing mark_expired_in_toml()
    +--> Requires: Existing LifecycleStatus enum (add Retired variant)
    +--> Independent of: Discovery features
    +--> Enables: Long-term events.toml hygiene

[Proposal Workflow (TS-4)]
    |
    +--> Requires: Existing candidate appending (already works)
    +--> Requires: Existing ConfigReloader (detects approval edits)
    +--> Enhances: Operator experience via structured logs + metrics
    +--> Independent of: Which venues participate in matching

[Live Subscription Management (TS-5)]
    |
    +--> Requires: Existing ConfigReloader (triggers on approval)
    +--> Requires: Existing EventRegistry.refresh() (knows new instruments)
    +--> Requires: Venue WebSocket clients to expose subscribe/unsubscribe (NEW, HIGH complexity)
    +--> Depends on: Proposal Workflow (TS-4) for the approval trigger
```

### Dependency Notes

- **Polymarket Structured Discovery (TS-1) is the hardest new work.** Deribit and Kalshi discovery already work with structured API fields. Polymarket is the gap because its API lacks machine-readable strike/direction/asset fields.
- **Expiry Date Alignment (TS-2) unlocks real matching.** Without tolerance, the exact-date requirement will miss most real cross-venue matches. This is a surgical change to `find_cross_venue_candidates()`.
- **Event Retirement (TS-3) is fully independent** of discovery features. Can be built in any order. Provides operational hygiene.
- **Proposal Workflow (TS-4) is largely already built.** The candidate appending, structured logging, and config reload path exist. Enhancements are incremental.
- **Live Subscription Management (TS-5) is the highest-risk feature.** Requires changes to all three venue WebSocket clients to support runtime subscription changes. Each venue has different subscription semantics (Deribit uses JSON-RPC subscribe/unsubscribe, Kalshi uses a different message format, Polymarket CLOB has its own subscribe model).

### Build Order Recommendation

```
Phase 1 (independent, immediate value):
  Track A: Event Retirement & Cleanup (TS-3) -- operational hygiene, low risk
  Track B: Proposal Workflow Enhancement (TS-4) -- better notifications, low risk

Phase 2 (the matching upgrade):
  Polymarket Structured Discovery (TS-1) -- regex parser for groupItemTitle
  Expiry Date Alignment (TS-2) -- tolerance window for cross-venue matching
  These two together enable three-venue automated candidate proposals.

Phase 3 (the hardest piece, gated by validation of Phase 2):
  Live Subscription Management (TS-5) -- dynamic subscribe/unsubscribe
  This requires changes across all three venue client modules.
  Consider: is restart-on-approval acceptable for v1.2 MVP?
    If yes, defer TS-5 to v1.3 and rely on existing SIGHUP config reload.
```

---

## MVP Recommendation

### Must Build (eliminates manual events.toml curation for most cases)

1. **Polymarket Structured Discovery (TS-1)** -- Without this, Polymarket cannot participate in automated matching. The system remains limited to Deribit+Kalshi auto-matching with manual Polymarket curation. This is the primary gap.

2. **Expiry Date Alignment (TS-2)** -- Without tolerance matching, exact expiry dates will prevent most real cross-venue matches. Venues use different expiry conventions (Friday vs end-of-month). This is a small code change with large impact.

3. **Proposal Workflow Enhancement (TS-4)** -- The approval workflow already works at a basic level. Enhancement to structured logs and Prometheus metrics makes it production-ready for ongoing operation.

4. **Event Retirement (TS-3)** -- Unapproved expired candidates and long-expired events must be cleaned up. Without this, events.toml grows unboundedly during extended operation.

### Defer

- **Live Subscription Management (TS-5):** This is the highest-risk, highest-complexity feature. The alternative is acceptable for v1.2: approve a mapping, then SIGHUP or restart the system. New subscriptions take effect on restart/reload. Dynamic subscription management is a v1.3 enhancement once the discovery and matching pipeline is validated.

- **Candidate Quality Scoring (D-3):** Useful but not essential. The operator reviews proposals manually and can assess quality from the logged context. Build the data collection in v1.2; add scoring later.

- **Polymarket Question Pattern Library (D-2):** Start with hardcoded regex patterns for BTC price markets in v1.2. Make it configurable in a future iteration if pattern diversity increases.

- **Liquidity Pre-screening (D-3):** Requires additional API calls per candidate. Not worth the complexity for BTC markets which generally have decent liquidity across all three venues.

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Risk | Priority |
|---------|------------|---------------------|------|----------|
| Polymarket Structured Discovery (TS-1) | HIGH | HIGH | MEDIUM (regex fragility) | P1 |
| Expiry Date Alignment (TS-2) | HIGH | LOW | LOW | P1 |
| Event Retirement & Cleanup (TS-3) | MEDIUM | LOW | LOW | P1 |
| Proposal Workflow Enhancement (TS-4) | MEDIUM | LOW | LOW | P1 |
| Live Subscription Management (TS-5) | HIGH | HIGH | HIGH (3 venue clients) | P2 (defer to v1.3) |
| Match Confidence Scoring (D-3) | MEDIUM | MEDIUM | LOW | P2 |
| Discovery Health Monitoring (D-2) | LOW | LOW | LOW | P2 |
| Polymarket Pattern Library (D-2) | MEDIUM | MEDIUM | LOW | P3 |
| Venue Change Detection (D-2) | LOW | MEDIUM | LOW | P3 |
| Liquidity Pre-screening (D-3) | LOW | MEDIUM | LOW | P3 |

---

## Existing Code That Needs Modification vs New Code

Understanding what already exists is critical for accurate effort estimation.

### Modifications to Existing Code

| Module | What Exists | What Changes |
|--------|-------------|--------------|
| `events::discovery::discover_polymarket()` | Fetches all active markets, returns `Vec<PolymarketMarketInfo>` | Add crypto tag filtering, parse `groupItemTitle`/`question` for structured fields, return `Vec<DiscoveredInstrument>` instead of raw market info |
| `events::discovery::find_cross_venue_candidates()` | Exact four-field `MatchKey` (asset, strike, expiry, direction) | Change expiry matching from exact to tolerance-based |
| `events::discovery::MatchKey` | `expiry: NaiveDate` used for Hash/Eq | Either add tolerance to comparison or group by (asset, strike, direction) then post-filter expiry ranges |
| `config::events::LifecycleStatus` | Active, Expiring, Expired | Add `Retired` variant |
| `events::lifecycle::ContractLifecycleManager::poll_cycle()` | Discovers, matches, expires, warns, refreshes | Add retirement cleanup step, enhanced proposal logging, Polymarket inclusion in matching |
| `events::discovery::PolymarketMarketInfo` | conditionId, question, endDateIso, active, closed, tokens, category | Add groupItemTitle, groupItemThreshold, parent event fields |

### New Code Required

| Module | Purpose | Complexity |
|--------|---------|------------|
| Polymarket `groupItemTitle` parser | Extract (asset, strike, direction) from display labels | MEDIUM -- regex patterns, error handling, logging |
| Expiry tolerance matcher | Group instruments by (asset, strike, direction) then match expiry within window | LOW -- refactor of existing matching logic |
| Event archival writer | Move expired entries to archive file or remove after retention | LOW -- extension of existing toml_edit writers |
| Unapproved candidate cleanup | Auto-expire candidates past their expiry date | LOW -- date comparison in poll_cycle |
| Approval validation | Verify approved mappings have valid instruments | MEDIUM -- REST calls to verify instrument existence |

---

## Sources

### Venue API Documentation (HIGH confidence)
- [Polymarket Gamma API Overview](https://docs.polymarket.com/developers/gamma-markets-api/overview) -- endpoints, structure
- [Polymarket Gamma API Structure](https://docs.polymarket.com/developers/gamma-markets-api/gamma-structure) -- events/markets hierarchy
- [Polymarket Fetching Market Data](https://docs.polymarket.com/quickstart/fetching-data) -- query patterns
- [Kalshi API: Get Markets](https://docs.kalshi.com/api-reference/market/get-markets) -- structured fields including floor_strike, cap_strike, close_time, status
- [Kalshi API: Get Series](https://docs.kalshi.com/api-reference/market/get-series) -- series discovery with settlement sources
- [Kalshi API Changelog](https://docs.kalshi.com/changelog) -- field deprecations, new fields (volume on series Jan 2026, price_level_structure moved Oct 2025)
- [Deribit API Documentation](https://docs.deribit.com/) -- get_instruments endpoint

### Polymarket Market Structure (MEDIUM confidence -- verified via live API)
- [Polymarket Gamma API live endpoint](https://gamma-api.polymarket.com/) -- tested crypto tag filtering with `tag_id=21`
- [Polymarket Market Discovery Bot](https://deepwiki.com/frankomondo/polymarket-trading-bots-telegram/3.3-market-discovery-and-real-time-monitoring) -- real-world implementation of Gamma API discovery, CoinMarket struct patterns
- [Polymarket API Architecture](https://medium.com/@gwrx2005/the-polymarket-api-architecture-endpoints-and-use-cases-f1d88fa6c1bf) -- endpoint overview, data flow

### Cross-Venue Settlement & Matching (MEDIUM confidence)
- [How Kalshi and Polymarket Settle Event Contracts](https://defirate.com/prediction-markets/how-contracts-settle/) -- settlement divergence risks, dispute mechanisms
- [Prediction Market Arbitrage Guide 2026](https://newyorkcityservers.com/blog/prediction-market-arbitrage-guide) -- cross-venue matching challenges
- [Prediction Markets at Scale: 2026 Outlook](https://insights4vc.substack.com/p/prediction-markets-at-scale-2026) -- API maturation, developer tooling trends

### Event Lifecycle Patterns (MEDIUM confidence)
- [Event-Driven Finite State Machine for Distributed Trading](https://www.quantisan.com/event-driven-finite-state-machine-for-a-distributed-trading-system/) -- state machine patterns for trading systems
- [Signal Types, States, and Lifecycle](https://www.mql5.com/en/blogs/post/767493) -- PREVIEW/PENDING/ACTIVE/WIN/LOSS/EXPIRED lifecycle

---

*Feature research for: v1.2 Automated Event Management*
*Researched: 2026-02-26*
