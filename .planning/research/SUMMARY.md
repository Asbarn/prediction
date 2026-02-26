# Project Research Summary

**Project:** v1.2 Automated Event Management
**Domain:** Cross-venue prediction market arbitrage — automated event discovery, matching, and lifecycle management
**Researched:** 2026-02-26
**Confidence:** HIGH

## Executive Summary

The v1.2 milestone adds automated event discovery and lifecycle management to an existing production-grade Rust arbitrage signal generator. The critical finding from research is that the foundation is already substantially built: `discovery.rs` (981 lines), `lifecycle.rs` (593 lines), and `toml_writer.rs` (303 lines) already implement REST polling for all three venues, exact four-field cross-venue matching, TOML proposal writing, and registry refresh. The question is not "how to build discovery" but "what specific gaps remain." There are two primary gaps: (1) Polymarket structured field extraction — the API returns free-form question text with no machine-readable strike, direction, or asset fields, requiring regex-based parsing of `groupItemTitle`; and (2) feed subscription management — when an operator approves a mapping, the `EventRegistry` updates but the venue WebSocket supervisors never subscribe to the new instruments, so the approved event silently produces no market data.

The recommended approach is to address the simpler, higher-value gaps first and defer the architecturally complex feed subscription work. Polymarket structured discovery and expiry date tolerance matching (venues use different expiry conventions — Deribit on Fridays, Kalshi end-of-month) together unlock three-venue automated candidate proposals with minimal code changes. Event retirement and cleanup prevent unbounded `events.toml` growth during extended unattended operation. These three workstreams are independent and low-risk. Live subscription management is the right final piece: it requires modifying all three venue WebSocket supervisors and adding a new `SubscriptionManager` component, but the architecture is clear — use `watch::channel<Vec<String>>` to push updated instrument lists to supervisors, triggering graceful reconnects that pick up new subscriptions. An acceptable interim is SIGHUP/restart-on-approval if implementation complexity needs to be deferred to v1.3.

The dominant risks are false positive cross-venue matches (instruments that look equivalent but have different settlement semantics), race conditions between the TOML atomic writer and the file watcher on Windows, and expiry detection false positives (absence from a partial API response interpreted as instrument expiry). All three are mitigated by patterns already present or easily added: match confidence scoring with settlement time comparison, batched TOML writes per poll cycle, and requiring N consecutive absences before marking an instrument expired. The human approval gate (`approved = false` on all auto-discovered candidates) is a non-negotiable safety mechanism that must not be bypassed regardless of confidence scores.

## Key Findings

### Recommended Stack

The existing Rust dependency tree already covers virtually all v1.2 needs. The only new direct dependency is `strsim 0.11` for string similarity metrics (Jaro-Winkler, normalized Levenshtein) used in Polymarket question text matching. This crate is already compiled transitively via `clap_builder -> strsim 0.11.1`, so it adds zero binary size cost. All three venue discovery clients (`reqwest 0.12`), TOML writing (`toml_edit 0.22`), rate limiting (`governor 0.8`), async runtime (`tokio 1.x`), and structured logging (`tracing 0.1`) are already in place. The one-line Cargo.toml change is: `strsim = "0.11"`.

Full details in `.planning/research/STACK.md`.

**Core technologies:**
- `strsim 0.11`: Polymarket question similarity scoring — only new direct dependency; already compiled transitively
- `reqwest 0.12`: REST polling for all three venue discovery APIs — unchanged, already implemented
- `toml_edit 0.22`: Format-preserving TOML writes — preserves operator comments and formatting; already fully implemented
- `governor 0.8`: Per-venue rate limiting for discovery polls — share existing `VenueRateLimiter` instances, do not create separate ones
- `tokio::sync::watch`: Dynamic instrument list delivery to supervisors — key new usage pattern for SubscriptionManager

### Expected Features

Full details in `.planning/research/FEATURES.md`.

**Must have (table stakes):**
- Polymarket structured market discovery (TS-1) — current system is Deribit+Kalshi only for auto-matching; Polymarket requires regex parsing of `groupItemTitle` (e.g., "up 150,000" -> direction=Above, strike=150000)
- Expiry date tolerance matching (TS-2) — exact date matching misses most real cross-venue matches; venues differ by 3-7 days (Deribit Friday vs Kalshi end-of-month)
- Event retirement and cleanup (TS-3) — without pruning, `events.toml` grows to ~2600 entries per year; toml_edit parse time degrades linearly
- Proposal workflow enhancement (TS-4) — structured WARN-level logs with all matched fields, Prometheus gauges for pending proposal count, approval validation on config reload

**Should have (competitive):**
- Live subscription management (TS-5) — new approved events produce no market data without this; `SubscriptionManager` with `watch::channel` push to supervisors and graceful reconnect
- Match confidence scoring (D-3) — combine expiry alignment confidence, venue count, and settlement time difference into 0.0-1.0 score; prevents operator fatigue and rubber-stamping
- Discovery health monitoring (D-2) — per-venue failure counters; alert if a venue consistently fails discovery (distinct from feed health)

**Defer (v1.3+):**
- Live subscription management is acceptable to defer if SIGHUP/restart-on-approval is operationally tolerable; the architecture is designed for it but implementation is highest-risk
- Multi-asset discovery (ETH, SOL) — validate BTC automation first; keep `deribit_currencies = ["BTC"]`
- Polymarket question pattern library (D-2) — start with hardcoded regex for BTC price markets; make configurable later
- NLP/ML-based question parsing — never; regex covers the BTC binary market pattern space and NLP adds massive dependency weight
- Automatic approval of high-confidence matches — never for a system managing real capital; the `approved = false` gate is a deliberate safety mechanism

### Architecture Approach

The v1.2 architecture follows a sidecar-subscription pattern: a new `SubscriptionManager` background task reads from the same `watch::channel<AppConfig>` as the existing config subscriber, diffs the EventRegistry's active instrument sets per venue, and pushes updated `Vec<String>` lists to each venue supervisor via dedicated `watch::channel` senders. Supervisors detect `instruments_rx.changed()` inside their `tokio::select!` forwarding loops and trigger graceful reconnects. This approach avoids touching the hot path (Feeds -> SpreadEngine -> SignalEngine), preserves the `ContractLifecycleManager`/`EventRegistry`/`ConfigReloader` triad unchanged, and leverages the already-battle-tested supervisor reconnection infrastructure. The `events.toml` file remains the single source of truth — no in-memory-only state that would be lost on restart.

Full details in `.planning/research/ARCHITECTURE.md`.

**Major components:**
1. `events/subscription.rs` (NEW) — SubscriptionManager: watches config changes, diffs per-venue instrument sets, pushes updated lists via watch channels, emits subscription metrics
2. `feed/*/supervisor.rs` (MODIFIED, minor) — accept `watch::Receiver<Vec<String>>`, add `instruments_rx.changed()` branch to select!, break inner loop on change to reconnect with updated list
3. `feed/pipeline.rs` and `main.rs` (MODIFIED) — create watch channels, wire senders to SubscriptionManager and receivers to supervisors
4. `events/discovery.rs` (MODIFIED) — add Polymarket crypto tag filtering, parse `groupItemTitle` for structured fields, change `PolymarketMarketInfo` return to `DiscoveredInstrument`
5. `events/discovery.rs::find_cross_venue_candidates()` (MODIFIED) — change expiry matching from exact `NaiveDate` equality to tolerance-based window (configurable, default 7 days)

### Critical Pitfalls

Full details in `.planning/research/PITFALLS.md`.

1. **False positive cross-venue matches** — Instruments that appear equivalent but have different settlement semantics (different settlement times on same expiry date, floating-point strike normalization drift). Avoid by adding settlement time comparison to candidate proposals, emitting WARN-level structured logs with raw venue data for operator verification, and tracking proposal vs approval rates via Prometheus counters.

2. **Feed subscription gap on approval** — EventRegistry updates when `approved = true` is set but no venue WebSocket subscribes to the new instruments; approved events silently produce no market data. Avoid by implementing SubscriptionManager (Phase 4) or as an interim: add a diagnostic metric for "approved events with no recent snapshots" and document that restart picks up new subscriptions.

3. **TOML write/file-watcher race condition** — Lifecycle manager and ConfigReloader both update the EventRegistry independently; on Windows, atomic rename produces DELETE + RENAME events that the debouncer may fire between. Avoid by batching all TOML modifications per poll cycle into one write, then refreshing registry once; treat double-refresh as harmless (idempotent) but log both sources distinctly.

4. **Stale API data causing false expirations** — Partial Deribit API responses (timeout mid-read) cause active instruments to appear absent and get marked expired. Avoid by requiring N consecutive absences (3+ polls) before expiry transition, validating response completeness (instrument count drop >20% = suspect), and checking that the expiry date has actually passed before writing `status = "expired"`.

5. **Polymarket discovery gap producing incomplete automation** — Without structured field extraction, all auto-discovered events are Deribit+Kalshi only. The regex approach on `groupItemTitle` covers the BTC binary market pattern space but is fragile to Polymarket format changes. Mitigate with logging of extraction failures and keeping BTC-only scope in v1.2.

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: Venue Discovery Hardening

**Rationale:** The discovery polling infrastructure exists but has production deficiencies: per-component rate limiters instead of shared ones, absence-based expiry detection without consecutive-absence guards, and no response completeness validation. These must be fixed before adding new complexity on top.

**Delivers:** Stable, production-safe discovery polling across all three venues; shared rate limiters; response completeness validation; N-consecutive-absence expiry guard; batched TOML writes per poll cycle.

**Addresses:** TS-3 (unapproved candidate expiration), TS-4 (structured proposal logs)

**Avoids:** Pitfall 3 (rate limit exhaustion from independent rate limiters), Pitfall 4 (false expirations from partial API responses), Pitfall 2 (TOML write race condition via batched writes)

### Phase 2: Cross-Venue Matching Upgrade

**Rationale:** Expiry tolerance matching (TS-2) is a surgical change to `find_cross_venue_candidates()` with high impact — without it, most real cross-venue matches are missed because venues use different expiry date conventions. Polymarket structured discovery (TS-1) is the hardest new work but the most valuable for full automation. These two belong together because both feed into the same candidate generation pipeline.

**Delivers:** Three-venue automated candidate proposals; expiry tolerance window (configurable, default 7 days); Polymarket `groupItemTitle` regex parser; match confidence scoring; settlement time comparison in proposals; WARN-level structured proposal logs.

**Uses:** `strsim 0.11` (Jaro-Winkler for Polymarket question confidence scoring); existing `discover_polymarket()` (add tag_id=21 filter, structured field extraction)

**Implements:** Modified `MatchKey` comparison, new Polymarket parser, confidence composite score on `CandidateMapping`

**Avoids:** Pitfall 1 (false positive matches via confidence scoring and settlement time logging), Pitfall 5 (Polymarket gap — partial mitigation via regex extractor)

### Phase 3: Lifecycle Integration and Cleanup

**Rationale:** Event retirement (TS-3) and full lifecycle coordination are independent of discovery features but critical for long-term operation. Without archival, `events.toml` grows unboundedly. This phase also adds the intermediate `expiry_detected` status, Prometheus coverage metrics, and approval validation on config reload.

**Delivers:** Expired event archival (entries >7 days past expiry moved to `events_archive.toml`); unapproved candidate auto-expiration; `Retired` lifecycle status variant; EventRegistry index skip for expired entries; coverage gauges (`lifecycle_events_coverage{venues="2|3"}`); approval validation (verify instrument still active on venue before activating mapping).

**Addresses:** TS-3 (full event retirement), TS-4 (Prometheus metrics for pending proposals)

**Avoids:** Pitfall 7 (TOML file growth), Pitfall 4 (intermediate `expiry_detected` state as safety buffer before committing to `expired`)

### Phase 4: Live Subscription Management

**Rationale:** This is the highest-risk, highest-complexity phase and the final gap that enables true fire-and-forget operation. Without it, newly approved events require a restart to begin receiving market data. Deferring to last ensures the discovery and matching pipeline is validated before complicating the feed layer. If SIGHUP/restart-on-approval is operationally acceptable, this phase can be deferred to v1.3.

**Delivers:** `events/subscription.rs` SubscriptionManager; `watch::Receiver<Vec<String>>` integration into all three venue supervisors; graceful reconnect on instrument list change; subscription activation/removal metrics; end-to-end flow: discover -> propose -> approve -> subscribe -> spreads.

**Implements:** Architecture components: SubscriptionManager, supervisor watch channels, main.rs wiring

**Avoids:** Pitfall 6 (feed subscription gap on approval), Anti-Pattern 1 (mpsc command approach — use reconnect instead)

### Phase Ordering Rationale

- Phase 1 before Phase 2: Shared rate limiters and batched TOML writes must be in place before high-frequency candidate generation from three-venue matching; otherwise rate exhaustion and race conditions compound.
- Phase 2 before Phase 3: Retirement and archival logic is simpler once the full discovery pipeline is working correctly; avoids building cleanup for a partially-correct system.
- Phase 3 before Phase 4: Subscription management reads from a stable, validated EventRegistry; Phase 3 adds approval validation that catches bad mappings before they trigger subscriptions.
- TS-5 (Live Subscription Management) is explicitly confirmed as deferrable: restart-on-approval has recovery cost LOW per the pitfalls research, and the architecture is designed to accommodate it naturally.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2 (Polymarket parser):** Polymarket `groupItemTitle` format stability and edge cases. The API is permissionless — format can change without notice. Sample live API responses for current BTC price market structure before finalizing regex patterns.
- **Phase 4 (Supervisor modifications):** Each venue WebSocket has different subscription semantics. Kalshi and Polymarket subscription message formats need verification against live API behavior before implementing. The reconnect approach sidesteps incremental subscription protocol differences but needs per-supervisor integration testing.

Phases with standard patterns (skip research-phase):
- **Phase 1 (Discovery hardening):** All patterns are well-established in the existing codebase. Rate limiter sharing, batched writes, and consecutive-absence tracking are straightforward Rust async patterns.
- **Phase 3 (Lifecycle cleanup):** Archival strategy and TOML manipulation via `toml_edit` follow the same patterns already proven in `toml_writer.rs`. No new technical territory.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | One new dependency (`strsim 0.11`); all others already in tree. Verified via Cargo dependency tree and crates.io. No version conflicts. |
| Features | HIGH (table stakes) / MEDIUM (Polymarket matching) | Deribit/Kalshi API behavior well-documented. Polymarket `groupItemTitle` format confirmed via live API but not guaranteed stable — permissionless market creation means format varies. |
| Architecture | HIGH | Based on direct codebase analysis of 32K+ LOC system. All component boundaries and data flows verified against actual source files. |
| Pitfalls | HIGH (integration pitfalls) / MEDIUM (venue API behavior under sustained load) | Integration pitfalls derived from direct codebase analysis. API behavior pitfalls based on documentation and reported edge cases; sustained-load behavior is inferred. |

**Overall confidence:** HIGH

### Gaps to Address

- **Polymarket `groupItemTitle` format coverage:** The regex approach is confirmed for BTC grouped price markets but the full range of question formats for edge cases is not exhaustively documented. During Phase 2 planning, sample live Polymarket crypto markets before finalizing regex patterns.
- **Windows atomic rename behavior under file watcher:** The `notify_debouncer_mini` interaction with `ReadDirectoryChangesW` on Windows (DELETE + RENAME sequence) is documented as a known issue in the notify-rs tracker. Verify the 500ms debounce is sufficient or add an explicit retry in the atomic write path during Phase 1.
- **Kalshi weekly ticker format:** `extract_kalshi_asset` strips known suffixes ("D", "MAXY"). If Kalshi introduces new ticker patterns, the parser may silently skip valid instruments. Monitor Kalshi changelog during implementation.
- **SubscriptionManager ordering guarantee:** The config subscriber (which refreshes EventRegistry) and SubscriptionManager both listen to the same `watch::channel<AppConfig>`. Registry must be refreshed before SubscriptionManager reads it. The recommended 50ms yield or `tokio::sync::Notify` solution needs validation in Phase 4 integration testing.

## Sources

### Primary (HIGH confidence)
- Direct codebase analysis: `src/events/discovery.rs` (981 lines), `src/events/lifecycle.rs` (593 lines), `src/events/toml_writer.rs` (303 lines), `src/events/registry.rs` (386 lines), `src/feed/pipeline.rs` (474 lines), `src/feed/deribit/supervisor.rs` (182 lines), `src/config/reload.rs` (118 lines), `src/main.rs` (791 lines)
- [strsim 0.11.1 API docs](https://docs.rs/strsim/0.11.1/strsim/) — confirmed Jaro-Winkler, normalized Levenshtein, Sorensen-Dice functions
- [Deribit API docs](https://docs.deribit.com/) — `public/get_instruments`: no auth, no pagination, ~1 req/s sustained
- [Kalshi API docs](https://docs.kalshi.com/api-reference/market/get-markets) — cursor-based pagination, RSA-PSS auth, Basic tier 20 reads/s
- [Kalshi rate limits](https://docs.kalshi.com/getting_started/rate_limits) — tier structure confirmed
- [Polymarket Gamma API](https://docs.polymarket.com/developers/gamma-markets-api/overview) — events/markets hierarchy, offset pagination

### Secondary (MEDIUM confidence)
- [Polymarket Gamma API live endpoint](https://gamma-api.polymarket.com/) — `tag_id=21` crypto filter tested, `groupItemTitle` format verified on live BTC price markets
- [notify-rs issue #382](https://github.com/notify-rs/notify/issues/382) — atomic rename race with file watcher debounce, Windows-specific behavior documented
- [Kalshi API Changelog](https://docs.kalshi.com/changelog) — `price_level_structure` moved Oct 2025, volume on series Jan 2026 — confirms active field changes

### Tertiary (LOW confidence)
- Sustained Polymarket Gamma API rate limits — Cloudflare-enforced, ~300 req/10s for `/books`; `/markets` endpoint limit is undocumented, inferred from general Cloudflare behavior
- Venue API cache staleness windows — the 20%-instrument-count-drop heuristic for detecting partial responses is based on operational inference, not documented API behavior

---
*Research completed: 2026-02-26*
*Ready for roadmap: yes*
