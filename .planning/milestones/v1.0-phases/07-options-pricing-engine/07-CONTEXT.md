# Phase 7: Options Pricing Engine - Context

**Gathered:** 2026-02-23
**Status:** Ready for planning

<domain>
## Phase Boundary

Extract implied probabilities from Deribit options data using Black-76 pricing, IV solving, multiple probability extraction methods (call spread replication primary), vol surface interpolation, and Greeks. Produces ImpliedProbability outputs with method, confidence, skew metadata, and attached Greeks. This phase delivers the quantitative engine that Phase 8 uses to compare options-implied probabilities against prediction market prices.

</domain>

<decisions>
## Implementation Decisions

### IV solver edge cases
- Newton-Raphson first, Brent's method fallback if NR fails to converge in N iterations
- Solve for bid IV, ask IV, AND mid IV separately — bid/ask IV feeds prob_bid/prob_ask fields; IV spread is a key confidence input
- Keep data flowing with honest confidence scores — never silently drop instruments. Clamp extreme IV values (configurable bounds), flag with reason code, let downstream decide
- Near-expiry cutoff: configurable TOML parameter, default 2 hours. Below cutoff, switch to intrinsic value pricing and flag clearly. Above cutoff, the existing tiered expiry warnings (48h/24h/6h from Phase 5) handle gradual degradation naturally
- Solver convergence behavior (NR iterations vs Brent fallback) is logged and feeds into confidence scoring

### Probability method ranking
- Always compute ALL methods in parallel: call spread replication (primary) + N(d2) (baseline). Log both, compare them
- Method disagreement is itself a confidence signal — feed into confidence scoring directly
- Call spread replication epsilon: use nearest liquid adjacent strikes on Deribit (not arbitrary offset). Configurable maximum epsilon beyond which fall back to N(d2). Log actual epsilon used per computation
- Skew: call spread inherently captures skew (uses real market prices). N(d2) gets explicit skew adjustment (strike vol minus ATM vol). Log skew magnitude as metadata on ALL methods for regime characterization
- Confidence scoring: weighted combination of 4 inputs, all configurable weights in TOML, output single 0.0-1.0 score:
  1. IV bid-ask spread (market certainty about vol at that strike)
  2. Book depth (thin books = manipulable prices)
  3. Method agreement (N(d2) vs call spread divergence craters confidence)
  4. Solver convergence (clean NR convergence = high, Brent fallback = suspect)
- Log all 4 confidence components alongside composite score for analysis

### Vol surface construction
- Linear interpolation between observed IV points + flat extrapolation beyond boundaries (v1 — simple, no negative vol, fast)
- Per-expiry smile only (no 2D strike+expiry surface). Call spread replication only needs same-expiry data
- Exclude strikes where IV bid-ask spread exceeds configurable threshold (hard filter). Linear interpolation doesn't support weighting — garbage strikes kink the surface
- Log excluded strikes so data quality gaps are visible
- Configurable minimum usable strikes (default 3). Below minimum, fall back to ATM flat vol with confidence reflecting degradation
- Configurable "good" strike count (default 5) for confidence tiers

### Greeks
- Compute delta, vega, and theta per-instrument. Skip gamma for v1 (execution/hedging concern, irrelevant without hedging)
- Per-instrument only, not position-aggregated (aggregation is Phase 8's concern)
- Greeks attached to ImpliedProbability struct (single output, easy downstream consumption)
- Theta cross-checks the carry cost model from Phase 6 — disagreement indicates a problem
- Underlying price: use specific futures contract matching the option's expiry (Black-76 is a futures model). Subscribe to relevant futures tickers. Fall back to index + carry estimate if specific futures illiquid, flag as lower confidence

### Claude's Discretion
- Exact Newton-Raphson convergence parameters (max iterations, tolerance)
- IV clamping bounds (min/max vol)
- Internal data structures for vol surface storage
- Exact confidence weight defaults
- Black-76 implementation details (d1/d2 computation, numerical stability)

</decisions>

<specifics>
## Specific Ideas

- "The IV spread itself is one of your best confidence inputs: a 2-vol-point bid-ask spread means tight markets; a 20-vol-point spread means the probability estimate could be off by several percent, which is wider than the arb you're looking for"
- "The basis between the perpetual and a 3-month future can be 1-3% in crypto, which directly biases your IV and implied probability if you use the wrong one"
- Theta cross-checks carry cost model — "if they disagree, something's wrong"
- "The goal is to keep data flowing through the pipeline with honest confidence scores, not to silently drop instruments that might be where the interesting dislocations are"
- Method disagreement between N(d2) and call spread replication is itself a confidence signal and useful research data for understanding which market regimes produce the best signals

</specifics>

<deferred>
## Deferred Ideas

- Gamma calculation — add when execution/dynamic hedging is needed
- SABR or parametric vol surface model — upgrade from linear interpolation in future iteration
- 2D vol surface (strike + expiry term structure) — add if term structure analysis proves valuable
- Portfolio-level Greeks aggregation — Phase 8 scope

</deferred>

---

*Phase: 07-options-pricing-engine*
*Context gathered: 2026-02-23*
