# Domain Pitfalls

**Domain:** Dynamic WebSocket subscription management and tech debt cleanup for an existing Rust cross-venue arbitrage signal generator (v1.3)
**Researched:** 2026-02-27
**Confidence:** HIGH (codebase analysis, venue API docs), MEDIUM (Windows file watcher edge cases, Polymarket unsubscribe reliability)

---

## Critical Pitfalls

Mistakes that cause data corruption, phantom signals, or require architectural rework.

### Pitfall 1: Stale Order Book State After Unsubscribe Produces Phantom Signals

**What goes wrong:**
When an instrument is unsubscribed (expired/retired), the subscription message is sent to the venue WebSocket, but the internal state tracking that instrument is NOT cleaned up. Specifically:

1. **SpreadEngine.latest** -- `HashMap<(String, Venue), MarketSnapshot>` retains the last snapshot for the unsubscribed instrument. If a new instrument maps to the same event_id (e.g., an expiry roll creates a new mapping for the same logical event), the spread engine pairs the NEW instrument's snapshot with the OLD instrument's stale snapshot. This produces a spread calculation using data from two different instruments, generating a phantom signal.

2. **DeribitProcessor.books** -- `HashMap<InstrumentId, InstrumentBook>` retains the order book for unsubscribed instruments. The book data is stale but not marked `is_stale` (staleness is only set on sequence gaps). If the instrument is re-subscribed later (e.g., operator re-approves after correcting a mapping), the processor compares the new stream's `prev_change_id` against the stale book's `last_change_id`, producing a sequence gap error and marking the book stale -- which is the RIGHT outcome but only by accident.

3. **KalshiProcessor.books** -- `HashMap<String, KalshiBook>` and **KalshiProcessor.last_exchange_ts** -- `HashMap<String, String>` retain state for unsubscribed tickers. Unlike Deribit's full-snapshot channel, Kalshi uses incremental deltas. Old book state causes incorrect depth if the ticker is re-subscribed.

4. **SpreadEngine.stats** -- `HashMap<String, RollingStats>` retains rolling statistics for the event. Stale rolling stats bias the dynamic threshold calculation for any new instrument associated with the same event_id.

**Why it happens:**
The current architecture was designed for static subscriptions set at startup. No component has a "remove instrument" path. Every HashMap that tracks per-instrument or per-event state grows monotonically.

**Consequences:**
- Phantom spread signals (false positives) from stale/mismatched data
- Incorrect threshold calculations from residual rolling stats
- Memory leak (minor: dozens of entries, not a real resource issue, but conceptually unclean)
- Sequence gap errors on re-subscribe that are confusing to debug

**Prevention:**
1. Add a `cleanup_instrument(venue, instrument_id)` method to each processor that removes its HashMap entries. Call this when an unsubscribe is confirmed.
2. Add a `cleanup_event(event_id)` method to SpreadEngine that removes entries from `latest` and `stats`.
3. The cleanup must happen AFTER the unsubscribe message is acknowledged by the venue, not before. Ordering: send unsubscribe -> wait for confirmation -> clean local state.
4. For Deribit, the `book.{instrument}.none.20.100ms` channel sends full snapshots, so re-subscribe gets a fresh book automatically. Mark the old book stale immediately on unsubscribe decision (before sending the message) as a belt-and-suspenders measure.

**Detection:**
- A spread computation where one leg's `exchange_timestamp` is more than 5 minutes older than the other leg's -- this should never happen with live data.
- SpreadEngine's `latest` HashMap growing without bound (add a Prometheus gauge for its size).
- Sequence gap errors on instruments that were recently unsubscribed and re-subscribed.

**Phase to address:** The phase implementing unsubscribe/cleanup must handle this. Cannot be deferred.

---

### Pitfall 2: Race Condition Between Config Reload and Subscription Reconciliation

**What goes wrong:**
The subscription reconciliation flow is: (1) config file changes, (2) `ConfigReloader` detects change and publishes new `AppConfig` via `watch::channel`, (3) `EventRegistry` refreshes, (4) a new reconciliation task compares current subscriptions against registry and sends subscribe/unsubscribe messages.

The race condition occurs between steps 3 and 4. The `EventRegistry.refresh()` method (line 75 of `registry.rs`) does a complete replace: clears all indexes, rebuilds from the new config. During the brief window where the registry is being rebuilt:

- **Scenario A (write lock contention):** The `forward_snapshots` function (pipeline.rs line 361) takes a `reg.read().await` lock on every incoming snapshot. If the reconciliation task holds a write lock while refreshing, all snapshot forwarding blocks. With 3 venues producing data at 100ms intervals, the `mpsc(1024)` buffers could fill during a slow refresh, causing backpressure up to the supervisors.

- **Scenario B (stale read during reconciliation):** The reconciliation task reads `registry.active_approved()` to determine what SHOULD be subscribed. If this read happens while the registry is mid-refresh (between `self.mappings = config.events.clone()` and `self.build_indexes()`), the indexes are empty but mappings is populated, producing incorrect results. However, since `refresh()` holds a `&mut self` (requires write lock) and `active_approved()` requires `&self` (read lock), Rust's RwLock prevents this. The actual risk is more subtle: the reconciliation task reads the registry, builds a diff, then sends subscribe messages. Between the read and the send, ANOTHER config reload could happen, invalidating the diff.

- **Scenario C (double reconciliation):** Two rapid config changes (e.g., operator approves one mapping then immediately approves another) could trigger two overlapping reconciliation tasks. Task 1 computes diff: subscribe instrument A. Task 2 computes diff: subscribe instruments A and B. Task 1 sends subscribe for A. Task 2 sends subscribe for A and B. Result: A is subscribed twice. Deribit ignores duplicate subscriptions (returns the channel in the result), but Polymarket and Kalshi behavior is undocumented for duplicate subscriptions.

**Why it happens:**
`watch::channel` delivers the LATEST value, not every intermediate value. Two rapid edits produce one notification with the final state, which is actually good. But the reconciliation task is a separate async task that is NOT atomic with the registry refresh. The `main.rs` config watcher (line 298) refreshes the registry, but there is no mechanism to trigger subscription reconciliation -- that does not exist yet and must be built.

**Consequences:**
- Missed subscriptions (new approved instrument not subscribed because reconciliation computed diff from intermediate state)
- Duplicate subscribe messages (harmless for Deribit, undocumented for others)
- Temporary snapshot forwarding stalls during write-lock contention

**Prevention:**
1. Reconciliation must be driven by the SAME task that refreshes the registry, not a separate watcher. Pattern: config_rx.changed() -> refresh registry -> compute subscription diff -> send subscribe/unsubscribe. All in one sequential flow, under one write lock acquisition.
2. Use a "desired state" reconciliation model: compute the full desired subscription set from the registry, compare against actual subscription set, send only the diff. This is idempotent -- running reconciliation twice produces the same result.
3. Debounce reconciliation to avoid acting on intermediate states. The file watcher already debounces at 500ms. Add a second debounce of 1-2 seconds between registry refresh and subscription reconciliation to allow multiple rapid config changes to settle.
4. Add a Prometheus metric `subscription_reconciliation_total` and `subscription_reconciliation_errors` to detect drift.

**Detection:**
- Instruments in `active_approved()` that are NOT receiving snapshots (check via per-instrument message count metrics).
- Subscription reconciliation log entries appearing in rapid succession (<1s apart).
- SpreadEngine receiving snapshots for instruments not in the registry (unmapped instrument, filtered at pipeline.rs line 199).

**Phase to address:** Must be the FIRST thing implemented in the subscription management phase. The reconciliation mechanism is the foundation everything else builds on.

---

### Pitfall 3: Windows File Watcher Produces DELETE + RENAME Events That Race With Debouncer

**What goes wrong:**
This is a known concern from the project context. On Windows, atomic file writes (write to `.tmp`, rename to `.toml`) produce the following filesystem events from `ReadDirectoryChangesW`:

1. `MODIFY` or `CREATE` for the `.tmp` file (filtered out -- extension is "tmp")
2. `DELETE` for the old `.toml` file
3. `RENAME` for `.tmp` -> `.toml`

The `notify_debouncer_mini` uses a 500ms window. Events 2 and 3 may arrive within the same debounce window (collapsed into one notification -- safe) or straddle the debounce boundary (event 2 triggers a reload, event 3 triggers another reload). The danger: if the reload triggered by event 2 (DELETE) attempts to read `events.toml`, the file does not exist momentarily. The `load_config()` function (config/mod.rs line 70) calls `std::fs::read_to_string()` which returns `Err(NotFound)`, mapped to `ConfigError::ReadFile`. The error branch in the watcher (reload.rs line 98-103) logs the error and KEEPS the previous config. This is the correct behavior -- the system recovers.

HOWEVER: there is a subtle secondary risk. The v1.2 `ContractLifecycleManager` ALSO writes `events.toml` via `toml_edit`. If the lifecycle manager and the operator both modify `events.toml` within the same 500ms debounce window (operator edits in an editor, lifecycle manager appends a candidate), the following can happen:

1. Operator saves -> DELETE old, CREATE new (editor's atomic save)
2. Lifecycle manager reads the file, modifies, writes atomically -> DELETE new, CREATE newest
3. File watcher debounces and reloads "newest" -- but operator's changes that were in "new" may have been overwritten by the lifecycle manager which read before the operator's save landed.

This is a TOCTOU (time-of-check-time-of-use) race on the TOML file itself, independent of the file watcher.

**Why it happens:**
Multiple writers to the same TOML file without a coordination mechanism. The file watcher is read-only but surfaces the symptom.

**Consequences:**
- Operator's manual approval (`approved = true`) overwritten by lifecycle manager writing a candidate
- Config reload fails transiently (file not found during DELETE-RENAME gap) but recovers
- Subscription reconciliation sees stale config and does not subscribe the newly-approved instrument

**Prevention:**
1. The lifecycle manager should acquire a file lock before reading/modifying events.toml. On Windows, use `fs2::FileExt::lock_exclusive()` or equivalent. Any manual editor save would still race, but at least programmatic writers coordinate.
2. Better: the lifecycle manager should never write `events.toml` while reconciliation is in progress. Use a "config generation" counter incremented on each write. Reconciliation checks the generation before and after computing diff; if it changed, recompute.
3. The 500ms debounce is adequate for editor saves but may be too short for the lifecycle manager's batch TOML write + config reload + reconciliation chain. Consider increasing to 1000ms or using `notify_debouncer_full` which provides richer event metadata.
4. Add retry logic to `load_config()` for `ReadFile` errors: wait 100ms, retry once. This handles the DELETE-RENAME transient.

**Detection:**
- `config reload failed, keeping previous` log message (existing logging at reload.rs line 99)
- Operator approves an instrument in the TOML, but the approval disappears on the next lifecycle manager write
- `EventRegistry refreshed` log entries with LOWER mapping counts than expected

**Phase to address:** Should be addressed in the config hot-reload integration phase. The existing 500ms debounce is adequate for most cases but the TOCTOU race needs a file lock or coordination mechanism.

---

## Moderate Pitfalls

Mistakes that cause degraded functionality or require investigation but not architectural rework.

### Pitfall 4: Venue-Specific Unsubscribe Protocol Differences

**What goes wrong:**
Each venue has a different unsubscribe protocol, and the current codebase has NO unsubscribe path for any venue:

**Deribit:** Uses `public/unsubscribe` JSON-RPC method with a `channels` array. The response contains only successfully unsubscribed channels. The unsubscribe must specify the exact channel names (e.g., `["book.BTC-27JUN25-100000-C.none.20.100ms", "ticker.BTC-27JUN25-100000-C.raw", "trades.BTC-27JUN25-100000-C.raw"]`). For each instrument, 3 channels must be unsubscribed (book, ticker, trades). Forgetting one leaves a dangling subscription that continues producing data.

**Polymarket:** Uses an `"operation": "unsubscribe"` message with `"assets_ids"`. However, there is conflicting information about whether Polymarket actually supports unsubscribing. The official docs describe the message format, but some sources suggest that "Polymarket does not support unsubscribing from channel streams once subscribed." The safe path is to test this behavior before relying on it, and have a fallback plan (reconnect with new subscription set).

**Kalshi:** Uses `"cmd": "unsubscribe"` with `"sids"` (subscription IDs). This means the system must track the subscription ID returned when subscribing. The current `KalshiClient` does NOT track subscription IDs -- it sends subscribe requests with sequential `id` fields but does not parse the response to extract `sid`. This is a missing prerequisite for unsubscribe.

**Why it happens:**
Each venue designed their API independently. There is no standard WebSocket subscription management protocol. The current codebase treats subscriptions as fire-and-forget at connection time.

**Consequences:**
- Dangling Deribit subscriptions (3 channels per instrument) wasting bandwidth
- Polymarket unsubscribe silently failing, requiring reconnect as fallback
- Kalshi unsubscribe impossible without subscription ID tracking

**Prevention:**
1. For Deribit: Use `build_subscription_channels()` (channels.rs line 117) to compute all 3 channel names for the instrument, then send a single `public/unsubscribe` batch. Verify the response lists all 3 channels.
2. For Polymarket: Test unsubscribe behavior empirically. If it works, use it. If not, implement "reconnect with new subscription set" as a graceful degradation. Since Polymarket reconnection is fast (no auth needed), this is acceptable.
3. For Kalshi: Modify `KalshiClient.start()` to parse subscribe responses and track `sid -> (channel, market_ticker)` mapping. Store this in a shared data structure accessible by the supervisor. When unsubscribing, look up the `sid` for the ticker.
4. Each venue's unsubscribe should be wrapped in a timeout. If the venue does not confirm unsubscribe within 5 seconds, log a warning and proceed with local cleanup anyway.

**Detection:**
- After unsubscribe, data continues arriving for the unsubscribed instrument (check processor message routing)
- Kalshi error code 4 ("Missing sids in unsubscribe") or error code 7 ("Unknown subscription ID")
- Polymarket continues sending book updates for unsubscribed token_ids

**Phase to address:** Each venue's subscribe/unsubscribe implementation should be a separate plan within the subscription management phase, with Deribit first (simplest protocol), then Kalshi (needs sid tracking), then Polymarket (needs empirical testing).

---

### Pitfall 5: Subscribe to New Instrument Without Full Book Snapshot Initialization

**What goes wrong:**
When dynamically subscribing to a new instrument on an existing connection, the first message behavior differs by venue:

**Deribit:** The `book.{instrument}.none.20.100ms` channel sends full grouped snapshots every 100ms. The first message after subscribing IS a full snapshot (the `update_type` field is "snapshot" for full replacements). The current `InstrumentBook.apply_snapshot()` (book.rs line 58) handles first messages correctly: when `last_change_id` is `None`, the sequence check is skipped. So dynamic subscribe works correctly for Deribit order books.

**Kalshi:** The `orderbook_delta` channel sends a snapshot message first, then incremental deltas. The `KalshiBook.apply_snapshot()` replaces all levels. This should work correctly for dynamic subscribe IF the first message is indeed a snapshot. This needs verification -- the channel may send only deltas if the server considers the subscription "resumed."

**Polymarket:** Polymarket does NOT maintain per-instrument order book state in the processor. Each message is a self-contained book event parsed directly into a MarketSnapshot. So dynamic subscribe should work without state initialization concerns.

**The real problem:** The DeribitProcessor and KalshiProcessor have no mechanism to receive "subscribe to this new instrument" commands from outside their processing loop. They are constructed with a fixed `raw_rx` channel and process whatever arrives. When a new instrument is subscribed at the WebSocket level (via the supervisor/client), messages start arriving on the same `raw_rx`. The processors will see messages for instruments they have no book state for and will create new book entries on first message. This is actually fine for the processors.

BUT: the forward_snapshots function (pipeline.rs line 361) does `reg.lookup_by_instrument()` which requires the new mapping to be in the registry. If reconciliation subscribes to the feed BEFORE the registry is refreshed, snapshots for the new instrument arrive but are dropped (lookup returns None, no event_id annotation). If the registry is refreshed BEFORE the subscription is sent, there is a window where the spread engine expects data for the event but none arrives.

**Why it happens:**
The ordering dependency: registry must be refreshed BEFORE subscription is sent, AND subscription must be sent BEFORE the spread engine can compute spreads. But the registry refresh and subscription send are currently in different components with no coordination.

**Consequences:**
- Dropped snapshots for newly subscribed instruments (briefly, until registry catches up)
- SpreadEngine waiting for data that is not yet arriving (benign -- it just doesn't compute spreads)
- Log noise: "unmapped instrument" messages during the transition window

**Prevention:**
1. Enforce ordering: refresh registry FIRST, then send subscribe. Since both happen in response to config change, do them sequentially in the same task.
2. Accept the transition window as benign. The spread engine already handles missing legs gracefully (returns early at line 219-223). A few seconds of missing spreads during subscription changes is acceptable for paper trading.
3. Add a `subscription_transition_active` flag that suppresses "unmapped instrument" log noise during the 5-second window after a reconciliation.

**Detection:**
- Burst of "unmapped instrument" log messages immediately after a config change
- Per-instrument message count drops to zero then recovers (gap in Prometheus metrics)

**Phase to address:** Same phase as the reconciliation mechanism. The ordering constraint is a design decision, not a separate implementation task.

---

### Pitfall 6: Supervisor Does Not Support Dynamic Instrument Changes

**What goes wrong:**
All three supervisors (`DeribitSupervisor`, `PolymarketSupervisor`, `KalshiSupervisor`) are constructed with a fixed instrument list at startup:

- `DeribitSupervisor` stores `instruments: Vec<String>` (supervisor.rs line 31)
- `PolymarketSupervisor` stores `config: PolymarketConfig` which contains `assets: Vec<PolymarketAsset>` (client.rs line 66-71)
- `KalshiSupervisor` stores `config: KalshiConfig` which contains `market_tickers: Vec<String>` (client.rs line 109)

When the supervisor reconnects (connection drop + backoff), it creates a fresh client with the SAME instrument list it was constructed with. If a subscription change happened while the connection was live, the reconnection reverts to the original set.

More fundamentally: the supervisors have no communication channel for receiving "subscribe to X" or "unsubscribe from Y" commands. They are opaque async tasks that just forward messages.

**Why it happens:**
The supervisor pattern was designed for reliability (reconnection), not mutability. Adding a command channel to each supervisor requires changing the core async loop (the `tokio::select!` in the `run()` method).

**Consequences:**
- Reconnection reverts subscription set to startup config, losing dynamic changes
- No mechanism to send subscribe/unsubscribe without modifying supervisor internals

**Prevention:**
Two architectural approaches:

**Option A: Command channel to supervisor.** Add an `mpsc::Receiver<SupervisorCommand>` to each supervisor. Commands: `Subscribe(instruments)`, `Unsubscribe(instruments)`, `SetSubscriptions(full_set)`. The supervisor forwards these to the underlying client. On reconnect, the supervisor uses the LATEST subscription set, not the original. This is the cleaner approach but requires modifying all 3 supervisors and all 3 clients.

**Option B: Subscription management at the client level.** The clients already own the write half of the WebSocket. Add a command channel to the client's spawned task. The supervisor does not need to change -- it just creates clients with the current subscription set (read from a shared state). On reconnect, the supervisor reads the latest desired set from a shared `Arc<RwLock<Vec<String>>>` instead of its stored `instruments` field. This is simpler but couples the supervisor to shared mutable state.

**Recommendation:** Option A. The command channel pattern is more Rust-idiomatic and keeps ownership clear. The supervisor owns the command channel, the reconciliation task sends commands. The supervisor's reconnect path reads its own internal `current_instruments` field which is updated by command processing.

**Detection:**
- After a reconnection, the system subscribes to instruments that should have been unsubscribed
- Instruments added after startup never appear in feed data after a reconnection event

**Phase to address:** This is the core architectural change for v1.3. Must be implemented first, then subscribe/unsubscribe logic layers on top.

---

### Pitfall 7: Polymarket groupItemTitle Format Not Guaranteed Stable

**What goes wrong:**
The Polymarket discovery system (discovery.rs) parses question text using 3 hardcoded BTC price patterns to extract strike prices and direction. The `groupItemTitle` field from the Gamma API is the primary source for this parsing. If Polymarket changes their question format (e.g., from "Will Bitcoin be above $100,000?" to "BTC price > $100K?" or introduces localized number formatting), the parser silently fails and returns no discovered instruments.

This is particularly dangerous for v1.3 because the discovery system now drives subscription changes. If parsing breaks:
1. New instruments are not discovered
2. No new candidates are proposed
3. No new subscriptions are added
4. The system silently stagnates, trading only existing (potentially expiring) instruments

**Why it happens:**
String parsing is inherently fragile. The current approach was a deliberate design decision (PROJECT.md: "NLP/ML-based Polymarket question parsing -- regex sufficient for predictable BTC price patterns") that trades robustness for simplicity.

**Consequences:**
- Silent discovery failure with no error metrics (parser returns empty vec, which is a valid result)
- System runs on an ever-shrinking instrument set as events expire and no new ones are discovered
- No immediate alerting -- the absence of discovery is indistinguishable from "no new markets exist"

**Prevention:**
1. Add a Prometheus metric `discovery_parse_attempts_total` and `discovery_parse_failures_total` per venue. If the ratio exceeds a threshold (e.g., >50% failure), emit an alert.
2. Log a WARN when the Polymarket API returns events with question text that contains "BTC" or "Bitcoin" but the parser cannot extract a strike/direction. This catches format changes without false positives on non-BTC markets.
3. Add a golden test corpus of real Polymarket question strings that the parser must handle. When adding v1.3 features, run these tests to verify the parser still works.
4. Consider a fallback: if the Polymarket structured API (`tokens` endpoint) provides strike/direction metadata directly, use that instead of parsing question text.

**Detection:**
- `discovery_polymarket_instruments` Prometheus metric drops to zero
- `lifecycle_candidates_discovered` stops incrementing while other venues continue discovering
- Log search for "parsed 0 polymarket instruments" in the lifecycle manager

**Phase to address:** Not a v1.3 blocker but should be instrumented (metrics + logging) during this milestone to prevent silent stagnation.

---

### Pitfall 8: Tech Debt Cleanup Breaking Working Pipeline

**What goes wrong:**
The v1.3 milestone includes cleaning up 15 tech debt items from v1.0-v1.2. Tech debt cleanup often changes code paths that are currently working in production. Specific risks from the known tech debt:

1. **`iv_spread` always 0.0:** Fixing this changes the spread computation output. Any downstream consumer that has been calibrated against the current (broken) behavior will need recalibration. Paper trade historical data becomes incomparable pre/post fix.

2. **Expired test instrument removal:** If any test that uses the expired instrument is not updated, CI breaks. More subtle: if the expired instrument is in `events.toml` and the discovery system uses it as a "known" instrument to avoid re-proposing, removing it changes discovery behavior.

3. **Empty Kalshi default config:** Changing the default could affect Mock/Replay mode where Kalshi config might be absent. If the default changes from "no tickers" to something else, mock tests may fail.

4. **Options book depth hardcoded 0:** Changing this affects DeribitProcessor book state size. If the change introduces a bug in depth handling, ALL Deribit spreads are affected.

5. **Unused exact-match functions (v1.2):** Removing dead code is generally safe, but if any of these functions are used via `cfg(test)` or conditionally compiled code, removal breaks tests.

6. **expiry_confidence TOML field is write-only (v1.2):** Removing the write or adding a read changes serialization behavior. If `events.toml` files in the wild have this field and the deserializer does NOT have `#[serde(default)]` or `deny_unknown_fields`, adding it as a read field changes nothing. Removing the write changes the TOML output format.

**Why it happens:**
Tech debt items are individually simple but collectively touch many code paths. Each fix has a blast radius that may not be obvious. The risk is amplified because v1.3 also introduces subscription management, so bugs from tech debt cleanup could be attributed to the new feature, making debugging harder.

**Consequences:**
- CI failures from test breakage
- Subtle behavior changes in spread computation, threshold evaluation, or discovery
- Confusing debugging when subscription management bugs and tech debt bugs manifest simultaneously

**Prevention:**
1. **Separate tech debt from feature work in git history.** Each tech debt item should be its own commit. Never mix tech debt fixes with subscription management changes.
2. **Tech debt cleanup AFTER subscription management is complete and tested.** This ensures that any regression from tech debt can be cleanly bisected. The PROJECT.md already lists tech debt as a separate concern.
3. **Run the full test suite after each tech debt fix, before starting the next one.** Do not batch multiple tech debt fixes into a single "cleanup" commit.
4. **For behavior-changing fixes (iv_spread, book depth): add a log message** noting the behavior change, so replay comparisons are aware.
5. **Keep a "before/after" recording comparison.** Run the system with a recorded feed before and after each behavior-changing tech debt fix. Compare spread output JSONL to verify only the expected values changed.

**Detection:**
- Test failures in CI after a tech debt commit
- Spread JSONL output differs between replay runs before/after the fix in unexpected ways
- Paper trade metrics (hit rate, edge) shift discontinuously after tech debt deployment

**Phase to address:** Tech debt should be a SEPARATE phase from subscription management, executed after subscription management is verified working. Within the tech debt phase, behavior-changing fixes should come before behavior-preserving fixes so their impact can be measured.

---

## Minor Pitfalls

Issues that cause inconvenience or confusion but are self-correcting or low-impact.

### Pitfall 9: Deribit Rate Limiter Interaction With Subscribe/Unsubscribe Bursts

**What goes wrong:**
When reconciliation needs to subscribe to multiple new instruments or unsubscribe from multiple old ones, it sends a burst of subscribe/unsubscribe messages. Each message is rate-limited by `VenueRateLimiter` (governor-based, configured at 20 req/s for Deribit). If reconciliation needs to subscribe to 5 instruments (15 channel subscriptions if done individually, or 1 batch if done as a single `public/subscribe` call), rate limiting could delay the operation.

The current Deribit client already uses batch subscribe (client.rs line 110-116): a single `public/subscribe` with all channels. If dynamic subscribe follows the same pattern, one subscribe message covers all new instruments. But if subscribe and unsubscribe are sent separately, that is 2 messages, which is well within 20 req/s.

The real issue: during reconciliation, other rate-limited operations may be queued -- heartbeat test_request responses (which are correctly EXEMPT from rate limiting per client.rs line 239), settlement polling, and discovery API calls all share the same `VenueRateLimiter`. A subscribe burst during active settlement polling could delay settlement checks or vice versa.

**Why it happens:**
The rate limiter is shared across all operations for a venue. This is correct for global rate limiting but means that subscription management contends with settlement and discovery operations.

**Prevention:**
1. Use batch subscribe/unsubscribe (single message with multiple channels/tickers) to minimize the number of rate-limited operations.
2. Subscribe/unsubscribe is infrequent (only on config changes) while settlement polling is periodic. The contention window is tiny. Monitor but do not over-engineer.
3. If contention becomes an issue, use a priority queue in the rate limiter (subscribe/unsubscribe gets higher priority than settlement polling).

**Detection:**
- Subscribe confirmation takes >1 second (should be near-instant at 20 req/s)
- Settlement polling logs showing delayed execution concurrent with subscription changes

**Phase to address:** Likely not an issue in practice. Monitor during testing.

---

### Pitfall 10: EventRegistry.refresh() Behavior With New EventMapping Entries

**What goes wrong:**
This is called out in the milestone context as needing verification. `EventRegistry.refresh()` (registry.rs line 75) does:

```rust
self.mappings = config.events.clone();
self.instrument_index.clear();
self.event_index.clear();
self.build_indexes();
```

This is a full replace. New entries in `config.events` (from discovery appending candidates) are correctly picked up. Removed entries (from archival) are correctly dropped. The indexes are rebuilt from scratch.

The subtle issue: between `self.instrument_index.clear()` and `self.build_indexes()`, a concurrent read via `lookup_by_instrument()` would find nothing. But since `refresh()` requires `&mut self` and is protected by `RwLock`, no reads can be concurrent. This is safe.

HOWEVER: the test at registry.rs line 353-385 (`refresh_rebuilds_indexes`) only tests the case where the refresh REPLACES all mappings. It does not test the case where refresh ADDS new mappings while keeping old ones. If the EventsConfig has duplicate instrument IDs across mappings (e.g., two events with the same Deribit instrument but different event_ids), `build_indexes()` (line 112-133) overwrites the index entry with the LAST mapping's index. This means the first mapping becomes unreachable by instrument lookup.

**Why it happens:**
The instrument_index is a `HashMap<(Venue, String), usize>` -- one entry per (venue, instrument) pair. If two events share an instrument, only the last one is indexed.

**Consequences:**
- If the same instrument appears in two active mappings (shouldn't happen in normal operation but possible during transition), one mapping becomes invisible to the pipeline.
- Spreads are computed against the wrong event_id.

**Prevention:**
1. Add a validation check during `build_indexes()` that warns on duplicate (venue, instrument_id) entries.
2. The config validation (config/validation.rs) should reject events.toml with duplicate instrument IDs across active mappings.
3. During reconciliation, verify that the instrument being subscribed/unsubscribed maps to exactly one active event.

**Detection:**
- WARN log: "duplicate instrument index entry for (Venue, instrument_id), last mapping wins"
- Spread results with event_id that does not match the expected event for the instrument

**Phase to address:** Add the validation check during the reconciliation implementation. Low priority.

---

### Pitfall 11: Kalshi Subscription ID Tracking Not Implemented

**What goes wrong:**
The Kalshi WebSocket API requires subscription IDs (`sids`) for unsubscribing. The current `KalshiClient` (client.rs line 109-131) sends subscribe messages with sequential `id` fields but does NOT parse the subscribe response to extract the `sid` assigned by the server. Without `sid` tracking, unsubscribe is impossible via the standard API.

**Why it happens:**
The original implementation (v1.0) treated subscriptions as permanent (set once at connection time, never changed). There was no need to track subscription IDs because unsubscribe was not a feature.

**Consequences:**
- Cannot unsubscribe from Kalshi tickers without reconnecting
- Reconnect-to-unsubscribe is a viable workaround but causes a brief data gap

**Prevention:**
1. Modify `KalshiClient.start()` to parse subscribe responses and store `sid -> market_ticker` mapping.
2. Expose the mapping via a shared data structure or return it with the receiver channel.
3. If Kalshi's subscribe response format does not include a `sid` in the response to the subscribe command, fall back to reconnect-based subscription management (cancel the Kalshi supervisor's token, let it reconnect with the updated ticker list).

**Detection:**
- Kalshi error responses when attempting to unsubscribe (error code 4 or 7)
- Log: "Kalshi unsubscribe failed: no sid tracked for ticker X"

**Phase to address:** Investigate Kalshi's actual subscribe response format during implementation. If sid tracking is complex, reconnect-based approach is acceptable for the scale of this system (single-digit number of Kalshi tickers).

---

### Pitfall 12: Recording Files Continue Capturing Data for Unsubscribed Instruments

**What goes wrong:**
The `RecordingService` (recording/writer.rs) writes all raw messages to JSONL files. When an instrument is unsubscribed, messages for that instrument stop arriving, so recording stops naturally. But if unsubscribe fails (Pitfall 4) and data continues arriving, the recording captures data for instruments the system believes are unsubscribed. This creates a confusing recording corpus where "unsubscribed" instruments still have data.

More importantly: the recording files are organized per-venue (e.g., `recordings/deribit/`, `recordings/polymarket/`), not per-instrument. There is no mechanism to stop recording for a specific instrument while continuing for others on the same venue.

**Why it happens:**
Recording is at the venue level, not the instrument level. This was the correct design for a system with static subscriptions.

**Consequences:**
- Recording files contain data for instruments the system is not actively processing (wasted disk)
- Replay from these recordings would include data for unsubscribed instruments (confusing but not harmful -- the processor just ignores them)

**Prevention:**
1. Accept this behavior as benign. Recording everything is actually a feature -- it provides a complete historical record for debugging.
2. If disk usage is a concern, add a "recording filter" that skips messages for instruments not in the active subscription set. But this adds complexity and reduces the debugging value of recordings.
3. Log a count of "recorded but unsubscribed" messages per venue as a TRACE-level diagnostic.

**Detection:**
- Recording file sizes continue growing at the same rate after unsubscribing instruments
- Replay produces snapshots for instruments not in the current events.toml

**Phase to address:** Not a v1.3 concern. Recording everything is the safer default.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Supervisor command channels | Pitfall 6: supervisors have no command input | Add mpsc command channel to supervisor run loop; update reconnect path to use latest subscription set |
| Deribit subscribe/unsubscribe | Pitfall 4: must unsubscribe all 3 channels per instrument | Use build_subscription_channels() for consistent channel name generation; verify response |
| Kalshi subscribe/unsubscribe | Pitfall 11: no sid tracking for unsubscribe | Parse subscribe response; fall back to reconnect if sid not available |
| Polymarket subscribe/unsubscribe | Pitfall 4: unsubscribe may not be supported | Test empirically; implement reconnect-based fallback |
| Config-driven reconciliation | Pitfall 2: race between registry refresh and subscription send | Single-task sequential flow: refresh -> diff -> send; debounce |
| State cleanup after unsubscribe | Pitfall 1: stale book/snapshot data produces phantom signals | Explicit cleanup methods on processors and SpreadEngine; cleanup AFTER unsubscribe confirmed |
| Book initialization on subscribe | Pitfall 5: ordering between registry refresh and subscription send | Refresh registry BEFORE sending subscribe; accept brief transition window |
| Windows file watcher | Pitfall 3: DELETE+RENAME race during atomic write | Existing debounce handles it; add retry on ReadFile error |
| Polymarket question parsing | Pitfall 7: format changes break discovery | Add parse failure metrics; log unparseable BTC-related questions |
| Tech debt cleanup | Pitfall 8: behavior changes break working code | Separate commits per item; tech debt AFTER subscription management; before/after recording comparison |
| Rate limiter contention | Pitfall 9: subscribe bursts compete with settlement polling | Use batch subscribe; monitor but do not over-engineer |
| EventRegistry duplicate instruments | Pitfall 10: two events with same instrument | Add validation during build_indexes; reject duplicates in config validation |

---

## Integration-Specific Warnings

These pitfalls arise specifically from integrating subscription management into the EXISTING pipeline architecture.

### The Pipeline Is Immutable Post-Construction

The multi-venue pipeline (`run_live_multi_venue` in pipeline.rs) constructs all components at startup: supervisors, processors, forwarders. Every component is `tokio::spawn`ed and communicates via `mpsc` channels. There is NO mechanism to:
- Replace a supervisor with one that has different instruments
- Replace a processor with one that tracks different books
- Add or remove forwarders

This is the fundamental architectural constraint v1.3 must work within. The solution is NOT to replace components but to add command channels INTO existing components.

### The Watch Channel Is Single-Consumer for Subscription Changes

The `watch::Receiver<AppConfig>` in main.rs (line 290) is currently used only to refresh the EventRegistry. For v1.3, the same config change must ALSO trigger subscription reconciliation. There are two options:
1. Clone the receiver (watch receivers are cheaply clonable) and spawn a second subscriber for reconciliation
2. Extend the existing subscriber to do both (refresh + reconcile) sequentially

Option 2 is better because it guarantees ordering (refresh before reconcile).

### Shutdown Token Hierarchy Must Include New Components

The existing shutdown hierarchy: `shutdown_token` (root) -> `venue_cancel` (child per venue) -> individual tasks. Any new reconciliation task or subscription manager must use a child token of the root, ensuring it shuts down before the pipeline components it communicates with.

---

## Sources

- Deribit API `public/unsubscribe`: [Deribit API Reference](https://docs.deribit.com/api-reference/subscription-management/public-unsubscribe)
- Polymarket WebSocket overview: [Polymarket WSS Documentation](https://docs.polymarket.com/developers/CLOB/websocket/wss-overview)
- Kalshi WebSocket connection: [Kalshi WebSocket Docs](https://docs.kalshi.com/websockets/websocket-connection)
- Kalshi WebSocket quick start: [Kalshi Quick Start WebSockets](https://docs.kalshi.com/getting_started/quick_start_websockets)
- Deribit market data best practices: [Deribit Support](https://support.deribit.com/hc/en-us/articles/29592500256669-Market-Data-Collection-Best-Practices)
- File watcher debouncing in Rust: [OneUpTime Blog](https://oneuptime.com/blog/post/2026-01-25-file-watcher-debouncing-rust/view)
- Windows FileSystemWatcher race conditions: [Deno Issue #13035](https://github.com/denoland/deno/issues/13035)
- notify crate docs: [docs.rs/notify](https://docs.rs/notify)
- tokio-tungstenite: [GitHub](https://github.com/snapview/tokio-tungstenite)
- Codebase analysis: All source file references cite exact line numbers from the prediction project codebase as read during research
