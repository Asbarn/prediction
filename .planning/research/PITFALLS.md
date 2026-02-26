# Pitfalls Research

**Domain:** Adding automated event discovery and cross-venue matching to an existing Rust cross-venue arbitrage signal generator (v1.2)
**Researched:** 2026-02-26
**Confidence:** HIGH (integration pitfalls based on codebase analysis and existing architecture), MEDIUM (venue API behavior under sustained polling)

---

## Critical Pitfalls

### Pitfall 1: False Positive Cross-Venue Matches Creating Bad Trading Signals

**What goes wrong:**
The discovery system matches instruments across venues that appear identical but are semantically different, producing a candidate with `approved = false` that the operator approves without catching the mismatch. Once approved, the spread engine computes spreads between instruments that do not share the same underlying event, generating nonsensical arbitrage signals. Because the system runs unattended for weeks, a single false match can produce hundreds of bad signals before anyone notices.

Specific false positive vectors in this system:

1. **Expiry date mismatch by 1-3 days.** Deribit options expire on Fridays at 08:00 UTC. Kalshi crypto markets use `close_time` which may differ by hours or days from Deribit's settlement time. The current `MatchKey` uses `NaiveDate` from the expiry, so a Deribit option expiring Friday 2025-06-27 at 08:00 UTC and a Kalshi market closing Sunday 2025-06-29 at 23:59 UTC would produce different `NaiveDate` values and NOT false-match. However, if both resolve to the same `NaiveDate` but at different times, the system matches them despite having different settlement windows -- this IS a false positive because the underlying price can move significantly between the two settlement times.

2. **Strike normalization divergence.** Deribit returns `strike` as `f64` (e.g., `100000.0`), Kalshi returns `floor_strike`/`cap_strike` as `f64`. Both are converted via `Decimal::from_f64_retain()`. For round strikes (50000, 100000), this is fine. For fractional strikes or very large values, floating-point representation differences could cause two "same" strikes to produce different `Decimal` values, either creating false negatives (missing a match) or, worse, matching an instrument at $99999.99999 with one at $100000.

3. **Direction mapping errors.** The system maps Deribit `call` to `Direction::Above` and Kalshi `floor_strike` to `Direction::Above`. If Kalshi changes its market structure or introduces new strike types, the direction mapping could silently produce wrong matches. The `extract_kalshi_asset` function strips "D" and "MAXY" suffixes -- if Kalshi adds new ticker patterns (e.g., "KXBTCW" for weekly), the parser may extract the wrong asset or silently skip valid instruments.

**Why it happens:**
Exact four-field matching (asset + strike + expiry + direction) works well for the structured venues (Deribit and Kalshi) but relies on normalization being perfectly consistent across venues. The system has no confidence scoring -- a match is either exact or rejected. There is no human-in-the-loop verification beyond the `approved = false` flag, and operator fatigue means approval can become rubber-stamping.

**How to avoid:**
- Add a **match confidence score** to `CandidateMapping` that encodes how precisely the fields align. Full exact match across all four fields = 1.0. Match with expiry within 3 days = 0.7. Match with strike within 0.01% = 0.9. Only auto-propose candidates above a configurable confidence threshold.
- Add **settlement time comparison** to the matching logic. If the Deribit settlement time (08:00 UTC on expiry day) and the Kalshi close time differ by more than 24 hours, flag this explicitly in the proposed candidate's metadata so the operator can assess basis risk before approving.
- Emit a structured log at WARN level for every proposed candidate, including the raw venue data that was matched (exact strike values, exact expiry timestamps, exact instrument IDs). Make it easy to visually verify the match without opening the TOML file.
- Consider adding a **dry run** mode where the discovery system logs what it WOULD propose without writing to events.toml, allowing the operator to validate the matching logic before enabling writes.
- Add a Prometheus metric `lifecycle_candidates_proposed_total` (already partially exists as `lifecycle_candidates_discovered`) and a companion `lifecycle_candidates_approved_total`. A divergence (many proposed, few approved) suggests matching quality issues.

**Warning signs:**
- Spread engine suddenly produces spreads for a new event that are always extreme (>50% spread) or always near-zero -- suggests mismatched instruments.
- `lifecycle_candidates_discovered` metric spikes without corresponding operator approvals.
- Operator finds approved events where settlement outcomes diverge between venues (e.g., Deribit settles ITM but Kalshi settles OTM for the "same" event).
- The `events.toml` file grows rapidly with unapproved candidates, suggesting the matcher is too aggressive.

**Phase to address:**
Phase 1 (Venue Discovery) for the matching logic, Phase 2 (Cross-Venue Matching) for confidence scoring and validation.

---

### Pitfall 2: Race Condition Between TOML Writing and File Watcher Reload

**What goes wrong:**
The `ContractLifecycleManager` writes to `events.toml` via `atomic_write()` (write to `.tmp`, rename to `.toml`). Simultaneously, the `ConfigReloader` watches the config directory with `notify_debouncer_mini` (500ms debounce). Two separate race conditions exist:

1. **Double-trigger on atomic write.** The atomic write creates `events.toml.tmp` then renames it to `events.toml`. The file watcher sees TWO filesystem events: the temp file creation (which it filters out since it doesn't end in `.toml` -- but `events.toml.tmp` has a double extension, and the filter checks `e.path.extension() == "toml"` which returns `"tmp"` NOT `"toml"`, so this is actually safe). BUT: on Windows, `ReadDirectoryChangesW` may emit a DELETE event for the old `events.toml` followed by a RENAME event. The debouncer collapses these into one reload, but the timing matters -- if the reload reads the file between the delete and the rename, it reads nothing (file doesn't exist) or reads stale data.

2. **Concurrent writes within a single poll cycle.** `ContractLifecycleManager::poll_cycle()` can call `append_candidate()` multiple times in a loop (once per new candidate) and `mark_expired()` multiple times. Each call reads the TOML, modifies it, and writes it back. If two async tasks or even sequential calls within the same cycle interleave with the file watcher, the watcher can trigger a reload between writes, causing the `EventRegistry` to temporarily contain partial updates.

3. **Config reload vs. lifecycle manager's own registry refresh.** After modifying the TOML, the lifecycle manager calls `self.refresh_registry()` which reads the file and updates the `Arc<RwLock<EventRegistry>>`. But the `ConfigReloader`'s watch handler in `main.rs` also updates the same `Arc<RwLock<EventRegistry>>` when it detects the file change. Both paths acquire a write lock. If the file watcher fires between the lifecycle manager's TOML write and its `refresh_registry()` call, the registry gets refreshed twice -- harmless but wasteful. If the file watcher fires DURING `refresh_registry()`, the write lock serializes them correctly. But if the lifecycle manager writes two candidates and the watcher fires between them, the registry temporarily has only the first candidate.

**Why it happens:**
The system has two independent mechanisms updating the same state: (a) the lifecycle manager directly writes TOML and refreshes the registry, (b) the file watcher detects TOML changes and refreshes the registry. Neither knows about the other. This is a classic "two sources of truth" problem where the file is the shared medium but there is no coordination protocol.

**How to avoid:**
- **Option A (recommended): Lifecycle manager skips file watcher.** Have the lifecycle manager refresh the registry directly after all TOML modifications in a poll cycle are complete (it already does this). Ignore the redundant file watcher reload -- it is harmless because `EventRegistry::refresh()` is idempotent. Add a log message distinguishing "registry refreshed by lifecycle manager" from "registry refreshed by file watcher" so the operator can verify both paths.
- **Option B: Lifecycle manager inhibits file watcher.** Use a shared `AtomicBool` flag: lifecycle manager sets it before writing TOML, clears it after `refresh_registry()`. The file watcher checks the flag and skips reload if set. This is more complex and prone to bugs if the lifecycle manager panics between set and clear.
- **Batch TOML writes within a poll cycle.** Instead of calling `append_candidate()` per candidate (which does a read-modify-write per candidate), collect all modifications and apply them in a single TOML write at the end of the cycle. This reduces the window for races and is also more efficient.
- On Windows, add an explicit delay or retry in `atomic_write()` to handle the case where `rename()` fails because the file watcher has an open handle on the target file. Use `tokio::fs::remove_file()` before `tokio::fs::rename()` as a Windows-specific workaround.

**Warning signs:**
- `tracing::error!("config reload failed, keeping previous")` appears in logs shortly after discovery appends.
- `EventRegistry` shows fewer mappings than `events.toml` contains (partial reload).
- Two "registry refreshed" log messages appear within 1 second of each other (double refresh from lifecycle manager + file watcher).
- On Windows: "Access denied" or "file in use" errors during atomic rename.

**Phase to address:**
Phase 1 (Venue Discovery) for the initial write implementation, Phase 3 (Lifecycle Integration) for the full coordination between lifecycle manager and config reload.

---

### Pitfall 3: API Rate Limiting Exhaustion During Discovery Polling

**What goes wrong:**
Discovery polling hits venue API rate limits, causing either HTTP 429 responses (requests dropped) or, worse, temporary IP bans. This is particularly dangerous because:

1. **Deribit `public/get_instruments`** returns ALL active options for a currency. For BTC, this can be 1000+ instruments across multiple expiries and strikes. The endpoint has a sustained rate of ~1 request/second for public endpoints. If the discovery poll interval is 300 seconds (5 minutes), a single request per currency per cycle is fine. But if the system polls multiple currencies (BTC, ETH, SOL) or retries on failure, it can exceed limits.

2. **Kalshi markets endpoint** paginates with `limit=200` and requires multiple requests to fetch all open markets in a series. Each request requires RSA-PSS signed authentication. The Basic tier allows 20 reads/second, but the discovery polling shares the rate limit budget with the WebSocket feed's REST fallback and the settlement checker. If all three are active simultaneously, combined request rates can exceed the tier limit.

3. **Polymarket Gamma API** paginates with `limit=100` and the discovery code loops with incrementing `offset` until a short page is returned. Rate limits are enforced by Cloudflare at ~300 requests per 10 seconds for the `/books` endpoint, but the `/markets` endpoint used for discovery may have different limits. Under sustained polling, Cloudflare may throttle or block the IP entirely.

4. **Shared HTTP client.** The lifecycle manager creates its own `reqwest::Client`, but the settlement monitor and feed supervisors use separate clients. If all three are polling the same venue API simultaneously, the combined request rate from the same IP can trigger rate limits even though each individual component stays within its budget.

**Why it happens:**
Discovery is implemented as a polling loop with fixed intervals, without awareness of the rate limit budgets consumed by other system components (settlement polling, feed REST fallback). The rate limiting is enforced per-IP at the venue level, not per-component at the application level.

**How to avoid:**
- **Use the existing `VenueRateLimiter` infrastructure** from the feed pipeline. The lifecycle manager should share the same `VenueRateLimiter` instances that the feed supervisors and settlement monitor use, not create independent rate budgets. Pass `venue_rate_limiters` from `PipelineHandles` to the `ContractLifecycleManager`.
- **Add backoff on HTTP 429/503 responses.** If a discovery poll returns a rate limit error, exponentially back off that venue's next poll. Do NOT retry immediately.
- **Cache discovery results.** If Deribit's instrument list hasn't changed (same set of active instruments), skip the cross-venue matching step. Use a hash of instrument IDs to detect changes cheaply.
- **Stagger venue polls.** The current design polls all venues in the same `poll_cycle()` call. Stagger them across the cycle interval: poll Deribit at t=0, Kalshi at t=interval/3, Polymarket at t=2*interval/3. This spreads the API load and reduces the chance of simultaneous rate limit exhaustion.
- **Log rate limit responses explicitly.** Track `lifecycle_discovery_rate_limited_total` as a counter so the operator can see if rate limits are being hit.

**Warning signs:**
- `lifecycle_discovery_polls` counter increments but discovered instrument counts are 0 (requests are being rejected).
- HTTP 429 or 503 errors in venue discovery logs.
- WebSocket feed reconnections or settlement polling failures coinciding with discovery poll times (shared rate limit exhaustion).
- Polymarket discovery returns empty results (Cloudflare silently returning empty pages when throttled).

**Phase to address:**
Phase 1 (Venue Discovery) for rate limiter integration, all phases for ongoing monitoring.

---

### Pitfall 4: Stale Discovery Data Creating Phantom Matches or Missing Expirations

**What goes wrong:**
Venue APIs return data that is stale, cached, or inconsistent, and the discovery system treats it as ground truth. This manifests in two ways:

1. **Phantom instruments.** A venue API returns an instrument that has actually expired or been delisted but the API cache hasn't cleared. The discovery system proposes a match for an instrument that no longer trades, which if approved would result in a dead feed subscription producing no data. The spread engine would show permanent staleness for this event.

2. **Missing instruments.** A venue API's cache doesn't yet include a newly listed instrument. The discovery system fails to find a cross-venue match because one venue's data is ahead of the other. On the next poll cycle, the instrument appears and is matched -- but there is a discovery latency of one full poll interval (5-10 minutes) that could miss a trading opportunity.

3. **Expiry detection false positives.** The lifecycle manager marks mappings as expired when a Deribit instrument no longer appears in the `get_instruments` response. But if the Deribit API response is cached or the request fails partially (network timeout after partial response), the instrument appears missing and gets incorrectly marked as expired. This is especially dangerous because `mark_expired_in_toml()` is irreversible within the same poll cycle -- once marked expired, the instrument is gone until the operator manually fixes the TOML.

The current code in `lifecycle.rs` lines 328-335 checks: "if we discovered Deribit instruments AND this mapping's Deribit instrument is not in the discovered set, mark it expired." This logic is correct in principle but fragile: if the Deribit API returns a partial result (e.g., 500 of 1000 instruments due to a timeout), the other 500 instruments get incorrectly marked as expired.

**Why it happens:**
Discovery polling is inherently point-in-time. The system has no way to distinguish "this instrument does not exist" from "this API call failed to include this instrument." The absence of data is treated as evidence of absence.

**How to avoid:**
- **Require N consecutive absences before marking expired.** Do not mark an instrument as expired on a single poll failure. Track a "missing count" per instrument and only transition to expired after 3+ consecutive polls where the instrument is absent AND the poll itself was successful (full instrument list received).
- **Validate API response completeness.** For Deribit, track the total number of instruments returned per currency. If the count drops by more than 20% from the previous poll, treat the response as suspect and skip expiry detection for that cycle. Log a warning.
- **Add a `raw_expiry_timestamp` comparison.** Before marking an instrument as expired via absence-from-API, check if its `expiry` date has actually passed. If the expiry is in the future, the instrument should NOT be expired -- flag this as an API anomaly rather than a genuine expiry.
- **Make expiry marking reversible.** Instead of immediately writing `status = "expired"` to TOML, write `status = "expiry_detected"` (a new intermediate state). Only transition to `expired` after the operator confirms OR after the expiry date has passed. This adds a safety buffer.
- **Cache the previous poll's instrument set** and diff against the current poll. Log the diff (added/removed instruments) so the operator can verify that expirations are genuine.

**Warning signs:**
- A mapping with a future expiry date is marked as expired in events.toml.
- Discovery logs show wildly varying instrument counts between polls (e.g., 800 then 200 then 900).
- A mapping is marked expired and immediately re-proposed as a new candidate in the same or next poll cycle.
- Deribit roll logic fires for instruments that haven't actually expired yet.

**Phase to address:**
Phase 1 (Venue Discovery) for response validation, Phase 2 (Cross-Venue Matching) for absence-count tracking, Phase 3 (Lifecycle Integration) for the intermediate expiry state.

---

### Pitfall 5: Polymarket Discovery Gap -- Unstructured Questions Prevent Auto-Matching

**What goes wrong:**
The current system (correctly) defers Polymarket auto-matching because Polymarket market questions are free-form text (e.g., "Will Bitcoin be above $100,000 on June 27, 2025?") without structured `strike`, `expiry`, or `direction` fields. The discovery code fetches Polymarket markets for deactivation monitoring only. But this creates a permanent gap: new Polymarket markets for BTC price events can only be matched to Deribit/Kalshi by manual operator curation.

This means the system's cross-venue coverage is always Deribit+Kalshi only for auto-discovered events. Polymarket mappings require the operator to:
1. Notice that a new Deribit+Kalshi match was proposed.
2. Manually search Polymarket for the corresponding market.
3. Find the correct `condition_id` and `token_id`.
4. Edit events.toml to add the Polymarket venue.
5. Set `approved = true`.

This is exactly the manual process the v1.2 milestone is supposed to eliminate. Without Polymarket in the auto-match loop, the system achieves only partial automation.

**Why it happens:**
Polymarket's market creation is permissionless and uses free-form questions. There is no structured field extraction available from the Gamma API. Building a text parser for "Will BTC be above $X on DATE?" questions is error-prone and requires handling many question formats, languages, and edge cases.

**How to avoid:**
- **Accept the limitation for v1.2** but design the architecture to accommodate future Polymarket matching. The `CandidateVenues` struct already has an `Option<(String, String)>` for Polymarket -- auto-discovered Deribit+Kalshi candidates leave this as `None` and the operator can fill it in.
- **Implement a semi-automated Polymarket lookup.** When a Deribit+Kalshi candidate is proposed, also query the Polymarket Gamma API with keyword filters (e.g., `tag=crypto`, searching for the asset and approximate strike in question text). Log any plausible Polymarket markets alongside the proposed candidate so the operator has a shortlist to choose from.
- **Build a simple regex extractor for common BTC price questions.** Patterns like "Will Bitcoin be above/below $X" or "BTC above $X by DATE" cover a large fraction of crypto binary markets. Use this as a HINT, not an auto-match -- flag it as `polymarket_suggestion` in the log with LOW confidence.
- **Track coverage metrics.** Add `lifecycle_events_coverage{venues="2"}` and `lifecycle_events_coverage{venues="3"}` gauges so the operator can see how many active events have full 3-venue coverage vs. partial.

**Warning signs:**
- All auto-discovered candidates have `polymarket = None` (expected in v1.2 but worth tracking).
- Operator spends significant time manually finding Polymarket markets for proposed candidates.
- Some events run with only 2-venue spread computation, reducing arbitrage detection quality.

**Phase to address:**
Phase 2 (Cross-Venue Matching) for the keyword search hint, deferred to v1.3 or later for robust NLP extraction.

---

### Pitfall 6: Feed Subscription Not Updated After New Events Approved

**What goes wrong:**
When the operator approves a new event mapping by setting `approved = true` in events.toml, the `ConfigReloader` detects the change and refreshes the `EventRegistry`. However, the WebSocket feed subscriptions for each venue are established at startup and not dynamically updated. The `DeribitClient` is created with a fixed `instruments: Vec<String>`, the `PolymarketClient` subscribes to fixed `assets`, and the `KalshiSupervisor` subscribes to fixed `market_tickers`.

This means: a new event is discovered, proposed, approved, and the registry is updated -- but the venue feeds never subscribe to the new instruments. The spread engine sees the event in the registry but never receives market data for it, so no spreads are computed. The system silently appears to work but misses all opportunities for newly approved events.

**Why it happens:**
The v1.0/v1.1 system was designed for static configuration. Feed subscriptions are set once at startup. The hot-reload mechanism (file watcher + `watch::Receiver<AppConfig>`) was built for tuning parameters (thresholds, fees) not for structural changes like adding new instruments. The `main.rs` comment on line 279 ("config hot-reload: refresh EventRegistry on TOML changes") creates the impression that new events are fully integrated, but the EventRegistry refresh only affects the spread engine's event lookup -- not the underlying feed subscriptions.

**How to avoid:**
- **Design a subscription management layer** that bridges the EventRegistry and the venue feed supervisors. When the registry changes (new active+approved mapping added), the subscription manager sends subscribe messages to the relevant venue WebSocket connections.
- For **Deribit**: send `public/subscribe` for the new instrument's order book channel via the existing WebSocket connection. Deribit supports dynamic subscription without reconnection.
- For **Polymarket**: the CLOB WebSocket supports subscribing to new markets by sending a subscribe message with the new token_id. No reconnection needed.
- For **Kalshi**: the WebSocket supports subscribing to new market tickers via the `subscribe` command.
- **Use the `watch::Receiver<AppConfig>` pattern** already in main.rs. Add a subscriber that compares the old and new event sets, computes the diff, and sends subscribe/unsubscribe messages to each venue's supervisor via a command channel.
- **As a simpler interim solution:** trigger a graceful WebSocket reconnection when new events are approved. The supervisor reconnects and subscribes to the updated instrument list from the registry. This is less efficient but leverages existing reconnection infrastructure.
- **Add a diagnostic check:** the spread engine should emit a metric or log when it encounters an active+approved event for which it has never received a MarketSnapshot. This catches the "approved but not subscribed" gap.

**Warning signs:**
- New events appear in the EventRegistry (`EventRegistry.active_count()` increases) but spread logs show no computations for those events.
- `staleness_rejection` alerts fire for newly approved events (no data received, so all snapshots are stale from the "default old" timestamp).
- Operator approves events and expects signals but nothing happens -- system appears broken when it is actually just not subscribed.

**Phase to address:**
Phase 4 (Live Subscription Management) -- this is likely the most architecturally complex phase and must be designed before Phase 1 begins, even if implementation comes later.

---

### Pitfall 7: TOML File Growing Unbounded With Expired Events

**What goes wrong:**
Every discovery cycle can append new candidates and mark old events as expired, but nothing ever removes entries from events.toml. Over weeks of unattended operation:
- The TOML file grows with hundreds of expired event entries.
- TOML parsing becomes slower (the `toml_edit` crate must process the entire document for each modification).
- The `EventRegistry` builds indexes over all entries including expired ones, increasing memory usage and lookup time.
- Human readability of events.toml degrades -- the operator must scroll past hundreds of expired/rejected entries to find active ones.

At 50 events per week (across all strikes and expiries for BTC), the file would contain ~2600 entries after a year of operation.

**Why it happens:**
"Append is easy, removal is hard." Removing TOML array entries while preserving formatting and comments requires careful `toml_edit` manipulation. The system was designed for "add new, mark expired" without a garbage collection strategy.

**How to avoid:**
- **Add a periodic archive/prune step.** Every N days (configurable, default 7), move all `status = "expired"` events that expired more than 7 days ago to an `events_archive.toml` file. Keep only active, expiring, and recently-expired events in the main file.
- **Or: use a separate file for auto-discovered candidates.** Write proposals to `events_candidates.toml` instead of the main `events.toml`. When the operator approves a candidate, the system moves it to `events.toml`. This keeps the main config file clean and human-curated.
- **Set an upper bound on TOML file size.** If the file exceeds a configurable limit (e.g., 100 entries), log a warning prompting the operator to archive.
- **In the registry, do not index expired events.** The current `build_indexes()` iterates all mappings. Add a filter to skip expired entries from the instrument/event indexes (but keep them in the `mappings` vec for reference).

**Warning signs:**
- `events.toml` file size grows by more than 1KB per day.
- TOML parse time (measurable via tracing spans around `toml::from_str`) exceeds 10ms.
- `EventRegistry.mapping_count()` is significantly larger than `EventRegistry.active_count()`.

**Phase to address:**
Phase 3 (Lifecycle Integration) for the pruning/archival strategy.

---

## Technical Debt Patterns

Shortcuts that seem reasonable but create long-term problems.

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Read-modify-write TOML per candidate instead of batching | Simpler code, each write is self-contained | N TOML parses and file writes per poll cycle instead of 1. Risk of inconsistent intermediate states if file watcher fires mid-batch | Only during initial development. Must batch before running unattended |
| Fixed temporary filename for atomic write (`events.toml.tmp`) | Simple, predictable | If two concurrent writers use the same tmp path, one overwrites the other. Currently only lifecycle manager writes, but adding a CLI tool or manual edit workflow could cause conflicts | Acceptable while only lifecycle manager writes. Must use unique tmp names if adding concurrent writers |
| Polymarket excluded from auto-matching entirely | Avoids NLP complexity, reduces false positive risk | Permanently limits 3-venue coverage to operator-curated Polymarket matches. Reduces the value proposition of automated discovery | Acceptable for v1.2. Must revisit for v1.3 |
| Expiry detection based on single poll absence | Quick to implement, catches obvious expirations | False positives when API returns partial data. Can incorrectly expire active instruments | Never for production. Must require N consecutive absences or expiry date validation |
| Lifecycle manager creates its own `reqwest::Client` | No dependency on pipeline handles | Separate rate limit budget from feed and settlement components. Combined per-IP rate can exceed venue limits | Only if venue rate limits are generous. Must share `VenueRateLimiter` for production |
| No deduplication check before TOML append | Simpler write logic | If registry refresh races with append, the same candidate could be appended twice in different poll cycles | Never. Must check both registry and file content before appending |

## Integration Gotchas

Common mistakes when connecting discovery to the existing system.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Discovery + EventRegistry | Refreshing registry from file on every write instead of building a diff | Batch all TOML writes per poll cycle, then refresh registry once. Use `refresh()` which rebuilds indexes from scratch (already implemented correctly) |
| Discovery + ConfigReloader | Assuming ConfigReloader handles discovery changes | ConfigReloader handles parameter tuning (thresholds, fees). Structural changes (new events, new instruments) need subscription management that ConfigReloader does not provide |
| Discovery + Feed Subscriptions | Assuming EventRegistry refresh automatically subscribes venue feeds | EventRegistry is a lookup table only. Feed subscriptions are established at startup via `DeribitClient::new(instruments)`. New events need explicit subscription commands to running WebSocket connections |
| TOML Writer + File Watcher | Using `notify_debouncer_mini` with 500ms debounce and assuming a single reload per write | Atomic rename produces multiple filesystem events (delete + rename on Windows). Debouncer may collapse them correctly OR may fire between the delete and rename, causing a reload failure |
| Lifecycle Manager + Settlement Monitor | Both reading EventRegistry via `Arc<RwLock<>>` concurrently with lifecycle manager writing | RwLock correctly allows concurrent reads. But settlement monitor caches event IDs to track -- if lifecycle manager expires an event, settlement monitor may still poll it. Must handle gracefully |
| Discovery + BasisRiskCache | Newly discovered (unapproved) events have no settlement metadata, so no basis risk score | The cache only populates for `active_approved()` events. Unapproved candidates correctly excluded. But when a candidate is approved, the cache is only updated on the next lifecycle poll cycle -- there may be a gap where the spread engine computes spreads without risk adjustment |
| Expiry Detection + Deribit Rolls | Marking an instrument expired AND creating a roll candidate in the same cycle | The current code does this correctly (roll handling is inside the expiry detection block). But if the roll candidate shares a strike/expiry with an existing mapping, `filter_new_candidates` may skip it as a duplicate. Verify the event_id generation (asset-strike-expiry) produces unique IDs for the roll target |

## Performance Traps

Patterns that work at small scale but fail as usage grows.

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Full TOML reparse on every modification | Parse time grows linearly with file size. With 500 events, parsing takes ~50ms | Batch modifications per poll cycle. Consider keeping a parsed `DocumentMut` in memory and only serializing to file | After ~200 events in events.toml |
| Deribit `get_instruments` returns ALL options | For BTC alone, 1000+ instruments per request. Response is ~500KB-1MB of JSON | Cache the response and diff against previous. Only run matching on new instruments | Always present for BTC. Worse if adding ETH/SOL currencies |
| Polymarket pagination fetches ALL active markets | Gamma API has thousands of active markets across all categories. Discovery fetches all of them just to monitor deactivation | Add category/tag filters to the Polymarket query. Only fetch crypto-related markets (`category=crypto` if available, or `tag_id` filter) | Immediately -- Polymarket has 1000+ active markets, most irrelevant to crypto binary events |
| `EventRegistry::build_indexes()` iterates all mappings including expired | Index build time grows linearly with total entries (active + expired) | Skip expired entries when building instrument/event indexes. Keep a separate `all_mappings` vec for reference | After ~500 total mappings (active + expired + rejected) |
| `find_cross_venue_candidates()` is O(n) over all discovered instruments | HashMap grouping is fast but still allocates for every instrument | Pre-filter instruments by asset before grouping. For BTC-only, filter out non-BTC instruments before calling `find_cross_venue_candidates` | After adding multi-asset support (ETH, SOL) which multiplies the instrument count |

## Security Mistakes

Domain-specific security issues relevant to automated discovery.

| Mistake | Risk | Prevention |
|---------|------|------------|
| Discovery appends Kalshi tickers to events.toml which is committed to git | Reveals trading strategy (which strikes and expiries the system targets) | Ensure events.toml is in .gitignore. The current `.gitignore` does NOT explicitly list events.toml -- verify this |
| Lifecycle manager logs full Kalshi API credentials in error messages | Credential leakage in log files shipped to monitoring | Redact `KALSHI-ACCESS-KEY` and `KALSHI-ACCESS-SIGNATURE` from error messages. Only log the first 4 characters of the key ID |
| Proposed candidates with `approved = false` can be flipped by anyone with file access | Unauthorized event activation could subscribe feeds to unvetted instruments | Not a security concern for solo trader. Would need access controls if multi-user |
| Discovery polling reveals interest in specific markets to venue APIs | Venues could use this information for targeted pricing or market making | Minimal concern: Deribit `get_instruments` is a generic request (all options, not specific strikes). Kalshi `GET /markets` with `series_ticker` filter reveals interest in that series but not specific strikes |

## "Looks Done But Isn't" Checklist

Things that appear complete but are missing critical pieces.

- [ ] **Discovery polling:** Often missing retry/backoff on HTTP errors -- verify that a failed Deribit/Kalshi/Polymarket poll does NOT prevent subsequent polls in the same cycle (the current `match` with `tracing::warn!` is correct but verify the error is not propagated to abort the poll cycle)
- [ ] **Cross-venue matching:** Often missing deduplication -- verify that the same candidate cannot be appended to events.toml twice if two consecutive poll cycles discover the same instruments before the registry refreshes
- [ ] **TOML writing:** Often missing Windows-specific atomic rename handling -- verify `tokio::fs::rename()` works when the target file exists on Windows (it may fail with "Access denied" if another process has the file open)
- [ ] **Expiry detection:** Often missing the "API returned partial data" case -- verify that a short Deribit response (timeout, partial read) does not trigger false expirations for instruments not in the partial response
- [ ] **Deribit roll handling:** Often missing the "multiple roll targets" case -- verify behavior when multiple future expiries exist for the same strike (should roll to nearest future, not arbitrary)
- [ ] **Registry refresh:** Often missing the "parse failure recovery" case -- verify that if the updated events.toml is malformed (e.g., mid-write read), the system keeps the previous registry state (the current `Err(e) => tracing::error!` pattern is correct)
- [ ] **Feed subscription gap:** Often missing entirely -- verify that newly approved events actually receive market data, not just registry presence. Add a diagnostic metric for "approved events with no recent snapshots"
- [ ] **Polymarket deactivation:** Often missing the "reactivation" case -- verify that a Polymarket market marked as deactivated then reactivated is handled (currently only logs deactivation, doesn't write to TOML)

## Recovery Strategies

When pitfalls occur despite prevention, how to recover.

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| False positive match approved and generating bad signals | MEDIUM | Identify the bad event in events.toml, set `approved = false` or `status = "expired"`. SIGHUP or restart. Review and discard any paper trades generated for that event. Add the false match pattern to a blocklist |
| Race condition corrupts events.toml | LOW | The `.tmp` file or the last-known-good config from `ConfigReloader` serves as backup. If both are corrupted, reconstruct from git history or manual recreation. The registry keeps the last successfully parsed config in memory |
| Rate limiting causes missed discoveries | LOW | Reduce poll frequency, share rate limiters. Missed discoveries are caught on the next successful poll. No data loss, just latency |
| False expiry marks active instrument as expired | MEDIUM | Edit events.toml: change `status = "expired"` back to `status = "active"`. If a roll candidate was created, remove it to avoid duplicates. Restart or trigger config reload |
| TOML file grows too large | LOW | Archive expired entries to `events_archive.toml`. Or delete all `status = "expired"` entries manually. Registry refreshes automatically on file change |
| Feed not subscribed to new approved events | LOW | Restart the system. On restart, all active+approved events from the registry are used to build the initial subscription list. Alternatively, implement dynamic subscription (the real fix) |
| Polymarket market changes format/structure | MEDIUM | Polymarket discovery is deactivation-monitoring only. If the Gamma API changes, discovery fails gracefully (the response parsing returns empty). Update the `PolymarketMarketInfo` struct and response parsing |

## Pitfall-to-Phase Mapping

How roadmap phases should address these pitfalls.

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| False positive matches (Pitfall 1) | Phase 2: Cross-Venue Matching | Create synthetic test data with near-miss matches (strike off by 1 cent, expiry off by 1 day). Verify they are NOT matched. Create exact matches and verify they ARE matched. Test with real Deribit+Kalshi instrument lists |
| TOML write race condition (Pitfall 2) | Phase 1: Venue Discovery (initial), Phase 3: Lifecycle Integration (full) | Run lifecycle manager with fast poll interval (10s) alongside config watcher. Verify events.toml is never corrupted. Count registry refreshes and verify they match expected count (1 per poll cycle + 0-1 from file watcher) |
| API rate limiting (Pitfall 3) | Phase 1: Venue Discovery | Integrate `VenueRateLimiter`. Run discovery polling for 1 hour against real APIs. Monitor for any HTTP 429 responses. Verify WebSocket feed stability is unaffected by concurrent discovery polling |
| Stale discovery data (Pitfall 4) | Phase 2: Cross-Venue Matching + Phase 3: Lifecycle Integration | Simulate partial Deribit API response (mock returning 50% of instruments). Verify no false expirations. Simulate instrument appearing/disappearing across consecutive polls. Verify the absence-count mechanism works |
| Polymarket discovery gap (Pitfall 5) | Phase 2: Cross-Venue Matching (design), deferred implementation | Track `lifecycle_events_coverage` metric. After 2 weeks of operation, review how many events have 3-venue vs 2-venue coverage. Assess operator effort for manual Polymarket curation |
| Feed subscription gap (Pitfall 6) | Phase 4: Live Subscription Management | Approve a new event in events.toml while system is running. Verify that within 60 seconds, the spread engine produces computations for that event. Verify WebSocket subscriptions include the new instruments |
| TOML file growth (Pitfall 7) | Phase 3: Lifecycle Integration | Run system for simulated 30 days (fast-forward poll cycles). Verify events.toml stays under a size threshold. Verify TOML parse time remains under 10ms |
| Expiry detection false positive (Pitfall 4) | Phase 3: Lifecycle Integration | Mock a Deribit API failure mid-cycle. Verify no active instruments are incorrectly marked as expired. Verify recovery on next successful poll |

## Sources

- Deribit public/get_instruments endpoint: https://docs.deribit.com/api-reference/market-data/public-get_instruments
- Deribit rate limits (credit-based system, ~1 req/s sustained for public): https://support.deribit.com/hc/en-us/articles/25944617523357-Rate-Limits
- Deribit market data collection best practices: https://support.deribit.com/hc/en-us/articles/29592500256669-Market-Data-Collection-Best-Practices
- Kalshi rate limit tiers (Basic: 20 read/s, Advanced: 30, Premier: 100, Prime: 400): https://docs.kalshi.com/getting_started/rate_limits
- Polymarket Gamma API structure: https://docs.polymarket.com/developers/gamma-markets-api/gamma-structure
- Polymarket rate limits (Cloudflare-enforced, ~300 req/10s for /books): Scribd mirror of Polymarket docs
- notify-rs file watcher debounce behavior and atomic rename interaction: https://github.com/notify-rs/notify/issues/382
- Atomic rename race with fixed tmp filename: https://github.com/google-gemini/gemini-cli/issues/18504
- File watcher EINVAL on temporary file during atomic write: https://github.com/anthropics/claude-code/issues/15832
- Cross-venue arbitrage desync and exchange outage attacks: https://www.researchgate.net/publication/396142626_Cross-Venue_Manipulation_Arbitrage_Desyncs_and_Exchange_Outage_Attacks
- Codebase analysis: `events/discovery.rs` (4-field matching), `events/toml_writer.rs` (TOML append), `events/lifecycle.rs` (poll cycle, expiry detection, roll handling), `config/reload.rs` (file watcher), `main.rs` (startup wiring, subscription setup)

---
*Pitfalls research for: v1.2 Automated Event Management milestone*
*Researched: 2026-02-26*
