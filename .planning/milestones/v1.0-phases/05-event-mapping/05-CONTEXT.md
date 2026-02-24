# Phase 5: Event Mapping - Context

**Gathered:** 2026-02-22
**Status:** Ready for planning

<domain>
## Phase Boundary

Cross-venue instrument registry that maps equivalent contracts across Polymarket, Kalshi, and Deribit. Each mapping carries quantified settlement basis risk and lifecycle status. Enables downstream spread calculations to compare the right instruments. Spread calculation itself is Phase 6+.

</domain>

<decisions>
## Implementation Decisions

### Mapping granularity
- Hybrid approach: auto-discovery proposes candidate matches, user reviews and sets `approved = true` in events.toml
- Match fields: asset + strike + expiry + direction (all four must align)
- Strike matching is exact after normalization -- no fuzzy tolerance band
- Unapproved candidates are visible to downstream but carry a `pending` flag -- useful for monitoring potential opportunities before committing
- Discovery writes candidates to events.toml with `approved = false` plus a structured log entry

### Basis risk scoring
- Structured breakdown: separate scores per factor (settlement_time_risk, source_risk, criteria_risk) plus a composite score
- Settlement time risk: hours matter -- score linearly with time difference (even a few hours is meaningful)
- Settlement source risk: categorical weights per source-pair (index-index = 0, index-oracle = 0.5, oracle-oracle = 0.2, etc.) -- predefined in config
- Risk is annotation only -- all approved mappings generate signals regardless of risk level. No automatic suppression.

### Lifecycle discovery
- Periodic REST polling of each venue's instrument list API (no feed-driven detection)
- Poll interval configurable in TOML per venue -- user tunes based on experience
- Auto-append candidates to events.toml with `approved = false` plus log entry
- Flag novel/unmatched instruments separately so user can spot new opportunity types (new assets, event types)

### Expiry behavior
- Configurable warning thresholds in TOML: multiple tiers (e.g., 'caution' at 48h, 'warning' at 24h, 'critical' at 6h) each with different flags
- Deribit expiry rolls create a new candidate mapping with `approved = false` -- user reviews before it goes live (approved status does NOT carry over)
- Expired mappings archived in events.toml with `status = 'expired'` -- kept for historical reference, excluded from runtime queries
- Near-expiry warnings both annotate the mapping AND inflate the settlement_time_risk component -- downstream gets both the flag and a quantitative signal

### Claude's Discretion
- Exact TOML schema design for events.toml
- REST polling implementation details per venue
- Composite risk score aggregation formula
- Internal data structures for the runtime registry

</decisions>

<specifics>
## Specific Ideas

- Discovery module proposes matches by pattern, writes candidates to events.toml, user flips `approved = true` after reviewing -- this was an earlier architectural decision that should be preserved
- Novel/unmatched instruments logged separately from candidate matches for opportunity discovery

</specifics>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 05-event-mapping*
*Context gathered: 2026-02-22*
