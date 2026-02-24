---
phase: 05-event-mapping
verified: 2026-02-22T22:00:00Z
status: passed
score: 15/15 must-haves verified
re_verification: false
gaps: []
human_verification: []
---

# Phase 5: Event Mapping Verification Report

**Phase Goal:** Equivalent instruments across Polymarket, Kalshi, and Deribit are mapped together through a config-driven registry, with each mapping carrying quantified settlement basis risk and lifecycle status, enabling downstream spread calculations to compare the right instruments.
**Verified:** 2026-02-22T22:00:00Z
**Status:** PASSED
**Re-verification:** No - initial verification

## Goal Achievement

### Observable Truths

| #  | Truth | Status | Evidence |
|----|-------|--------|----------|
| 1  | Event mappings in events.toml carry approval status, lifecycle status, and structured venue fields | VERIFIED | config/events.toml has approved, status, [events.venues.deribit/polymarket/kalshi] |
| 2  | EventRegistry is queryable by (venue, instrument_id) and by event_id | VERIFIED | lookup_by_instrument and lookup_by_event_id both implemented with HashMap O(1) |
| 3  | Auto-discovery appends new candidate mappings with approved=false without destroying existing content | VERIFIED | append_candidate_to_toml uses toml_edit DocumentMut; 6 toml_writer tests confirm preservation |
| 4  | Only active, approved mappings returned by active_approved(); expired mappings excluded | VERIFIED | active_approved() filters approved == true && status == Active; test at registry.rs:257 |
| 5  | Each mapping has BasisRiskScore with settlement_time_risk, source_risk, criteria_risk, composite | VERIFIED | BasisRiskScore struct in risk.rs:62; compute_basis_risk computes all four fields |
| 6  | Settlement time risk is linear with hours of temporal mismatch using Deribit Friday 08:00 UTC | VERIFIED | settlement_time_risk = hours * time_per_hour; deribit_settlement_time() appends T08:00:00Z |
| 7  | Settlement source risk uses categorical weights from config | VERIFIED | SourcePair::from_sources() + config SourcePairWeights; tested in risk.rs:406 |
| 8  | Near-expiry mappings have inflated settlement_time_risk based on configurable tier thresholds | VERIFIED | check_expiry_warning() + inflate_risk_score() in risk.rs; 3-tier config in events.toml |
| 9  | Risk scores are annotation-only (no automatic signal suppression) | VERIFIED | Risk is logged only; no filtering of signals or registry entries based on score |
| 10 | Lifecycle manager periodically polls each venue REST API at configurable intervals | VERIFIED | ContractLifecycleManager.run() with per-venue Instant tracking; min_poll_interval_secs() |
| 11 | Newly discovered cross-venue matches appended to events.toml with approved=false | VERIFIED | poll_cycle calls filter_new_candidates + append_candidate_to_toml with approved=false |
| 12 | Expired instruments detected and archived with status=expired in events.toml | VERIFIED | mark_expired_in_toml() called in poll_cycle step 4; atomic write via .tmp rename |
| 13 | Deribit expiry rolls create new candidate with approved=false (approval does NOT carry over) | VERIFIED | handle_deribit_roll() in lifecycle.rs:430; creates fresh CandidateMapping with approved=false |
| 14 | Discovery runs in its own async task and never blocks snapshot pipeline | VERIFIED | tokio::spawn(lifecycle_manager.run()) in main.rs:169; separate from pipeline task |
| 15 | Novel/unmatched instruments flagged separately for opportunity discovery | VERIFIED | flag_novel_instruments() in discovery.rs:454; logged in poll_cycle step 3 |

**Score:** 15/15 truths verified

### Required Artifacts

| Artifact | Provided | Lines | Status | Details |
|----------|----------|-------|--------|---------|
| src/events/registry.rs | In-memory event registry with dual-index lookup | 378 | VERIFIED | Exports EventRegistry; HashMap index by (Venue,String) and event_id; 9 unit tests |
| src/events/toml_writer.rs | Format-preserving TOML append | 303 | VERIFIED | Exports append_candidate_to_toml, mark_expired_in_toml, CandidateMapping; 6 tests |
| src/config/events.rs | Extended EventMapping with approval, lifecycle, settlement | 306 | VERIFIED | Contains approved, LifecycleStatus, SettlementMetadata, RiskWeightsConfig, DiscoveryConfig |
| config/events.toml | Extended schema with risk_weights, discovery, expiry tiers | 87 | VERIFIED | All 4 config sections present, settlement metadata, approved=false candidate example |
| src/events/risk.rs | Basis risk scoring with three components | 603 | VERIFIED | Exports BasisRiskScore, compute_basis_risk, ExpiryWarning, check_expiry_warning; 19 tests |
| src/events/discovery.rs | Per-venue REST discovery and cross-venue matching | 981 | VERIFIED | Exports discover_deribit/kalshi/polymarket, DiscoveredInstrument, MatchKey; 14 tests |
| src/events/lifecycle.rs | ContractLifecycleManager periodic background task | 752 | VERIFIED | Full poll_cycle with all 7 lifecycle steps; 5 tests |

### Key Link Verification

| From | To | Via | Status | Evidence |
|------|----|-----|--------|----------|
| src/events/registry.rs | src/config/events.rs | from_config(config: &EventsConfig) | WIRED | registry.rs:28 pub fn from_config(config: &EventsConfig) |
| src/events/toml_writer.rs | toml_edit document | DocumentMut parse and append | WIRED | toml_writer.rs:2 imports DocumentMut; used at lines 51, 107 |
| src/events/risk.rs | src/config/events.rs | Uses RiskWeightsConfig, ExpiryThreshold, SettlementMetadata | WIRED | risk.rs:4 imports all three types; used throughout scoring functions |
| src/events/lifecycle.rs | src/events/discovery.rs | Calls per-venue discover functions each cycle | WIRED | lifecycle.rs:24 imports; lines 123/177/227 call discover functions |
| src/events/lifecycle.rs | src/events/toml_writer.rs | Format-preserving TOML write-back | WIRED | lifecycle.rs:29 imports; lines 409/416 call append_candidate and mark_expired |
| src/events/lifecycle.rs | src/events/registry.rs | Refreshes EventRegistry after changes | WIRED | lifecycle.rs:27 imports; line 511 calls registry.refresh(&config) |
| src/events/lifecycle.rs | src/events/risk.rs | check_expiry_warning + inflate_risk_score | WIRED | lifecycle.rs:28 imports all three; lines 373/385 call both functions |
| src/main.rs | src/events/lifecycle.rs | Spawns ContractLifecycleManager as background task | WIRED | main.rs:9 imports; line 169 tokio::spawn(lifecycle_manager.run()) |
| src/feed/pipeline.rs | src/events/registry.rs | Pipeline accepts EventRegistry pass-through | WIRED | pipeline.rs:26 imports EventRegistry; line 74 optional Arc parameter |

### Requirements Coverage

| Requirement | Description | Status | Supporting Truths |
|-------------|-------------|--------|-------------------|
| EVNT-01 | Config-driven event registry maps instruments across venues with structured fields | SATISFIED | Truths 1, 2, 3, 4 |
| EVNT-02 | Settlement basis analyzer produces basis_risk_score with time, source, criteria components | SATISFIED | Truths 5, 6, 7 |
| EVNT-03 | Expiry alignment quantifies temporal mismatch as basis risk (Deribit Friday 08:00 UTC) | SATISFIED | Truth 6 (deribit_settlement_time helper produces T08:00:00Z) |
| EVNT-04 | Contract lifecycle manager continuously discovers/detects/handles contracts | SATISFIED | Truths 10, 11, 12, 13, 14 |
| EVNT-05 | Contracts approaching expiry receive special handling flags and elevated risk scores | SATISFIED | Truth 8 (ExpiryWarning tiers with configurable inflation factors) |

All 5 requirements assigned to Phase 5 (EVNT-01 through EVNT-05) are SATISFIED.

Note: REQUIREMENTS.md traceability table still shows these as Pending (last updated 2026-02-21 before the phase executed). This is a documentation lag only, not a code gap.

### Anti-Patterns Found

None. All five event module files searched for:
- TODO/FIXME/XXX/HACK/PLACEHOLDER comments: 0 matches
- Empty or stub implementations: 0 matches
- Placeholder API handlers: 0 matches

### Human Verification Required

None required. All phase 5 behaviors are fully programmatically verifiable.

Verification commands run and passed:
- cargo build: 0 errors, 1 pre-existing dead_code warning unrelated to phase 5
- cargo run -- check-config: Configuration valid for all three TOML files
- cargo test: 215 tests pass (171 lib + 16 integration + 3 pipeline + 22 smoke + 3 doc)

### Gaps Summary

No gaps. All 15 observable truths verified. All 7 required artifacts exist, are substantive, and are wired. All 9 key links confirmed. All 5 phase requirements satisfied. Phase goal achieved.

---

_Verified: 2026-02-22T22:00:00Z_
_Verifier: Claude (gsd-verifier)_
