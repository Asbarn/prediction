# Phase 7: Options Pricing Engine - Research

**Researched:** 2026-02-23
**Domain:** Black-76 options pricing, implied volatility solving, probability extraction (call spread replication + N(d2)), vol surface interpolation, Greeks computation
**Confidence:** HIGH

## Summary

Phase 7 builds the quantitative engine that converts raw Deribit options market data into implied probabilities suitable for comparison against prediction market prices in Phase 8. The core pipeline is: Deribit MarketSnapshot -> IV solver (Newton-Raphson with Brent fallback) -> vol surface construction (per-expiry linear interpolation) -> probability extraction (call spread replication primary, N(d2) baseline) -> Greeks computation -> ImpliedProbability output with confidence scoring.

The project's existing codebase is well-positioned for this phase. Deribit's ticker channel already provides `mark_iv`, `bid_iv`, `ask_iv`, `underlying_price`, `underlying_index`, and full exchange-computed Greeks -- all parsed in `TickerData` (messages.rs). However, critically, only `mark_iv` and exchange Greeks are carried through to `MarketSnapshot`. The `bid_iv`, `ask_iv`, `underlying_price`, and `underlying_index` fields are parsed but discarded in the normalization layer. Phase 7 must either extend `MarketSnapshot` and `TickerState` to carry these fields, or build independent IV solving from raw order book mid-prices. The user's decision to solve for bid IV, ask IV, AND mid IV separately (for confidence scoring from IV spread) means we need our own solver regardless -- Deribit's exchange IV is useful for cross-validation but not sufficient.

The mathematical implementation is well-understood: Black-76 is a closed-form model with analytic Greeks. The `statrs` crate (v0.18.0, MSRV 1.65, verified compiling on project's Rust 1.92) provides the Normal CDF needed for d1/d2 calculations. The `roots` crate provides Brent's method, but given the simplicity of 1D root-finding with known analytic derivative (vega), hand-rolling Newton-Raphson + Brent fallback is recommended over external crate dependencies. The critical numerical edge cases are near-zero vega (deep OTM/ITM), near-expiry theta collapse, and negative time value -- all addressed by the user's decision to clamp extreme IV with reason codes and switch to intrinsic pricing below a configurable near-expiry cutoff.

**Primary recommendation:** Implement a `PricingEngine` module (`src/pricing/`) with four sub-components: (1) Black-76 pricer with hand-rolled Newton-Raphson + Brent fallback IV solver, (2) per-expiry vol smile via linear interpolation with configurable quality filters, (3) probability extractor computing both call spread replication (using real adjacent strikes) and N(d2) in parallel, (4) confidence scorer combining IV spread, book depth, method agreement, and solver convergence. Use `statrs` for Normal CDF. Use f64 for all internal pricing math (project convention at metrics/math boundaries). Output `ImpliedProbability` structs carrying probability, confidence, method, skew metadata, and Greeks.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Newton-Raphson first, Brent's method fallback if NR fails to converge in N iterations
- Solve for bid IV, ask IV, AND mid IV separately -- bid/ask IV feeds prob_bid/prob_ask fields; IV spread is a key confidence input
- Keep data flowing with honest confidence scores -- never silently drop instruments. Clamp extreme IV values (configurable bounds), flag with reason code, let downstream decide
- Near-expiry cutoff: configurable TOML parameter, default 2 hours. Below cutoff, switch to intrinsic value pricing and flag clearly. Above cutoff, the existing tiered expiry warnings (48h/24h/6h from Phase 5) handle gradual degradation naturally
- Solver convergence behavior (NR iterations vs Brent fallback) is logged and feeds into confidence scoring
- Always compute ALL methods in parallel: call spread replication (primary) + N(d2) (baseline). Log both, compare them
- Method disagreement is itself a confidence signal -- feed into confidence scoring directly
- Call spread replication epsilon: use nearest liquid adjacent strikes on Deribit (not arbitrary offset). Configurable maximum epsilon beyond which fall back to N(d2). Log actual epsilon used per computation
- Skew: call spread inherently captures skew (uses real market prices). N(d2) gets explicit skew adjustment (strike vol minus ATM vol). Log skew magnitude as metadata on ALL methods for regime characterization
- Confidence scoring: weighted combination of 4 inputs, all configurable weights in TOML, output single 0.0-1.0 score:
  1. IV bid-ask spread (market certainty about vol at that strike)
  2. Book depth (thin books = manipulable prices)
  3. Method agreement (N(d2) vs call spread divergence craters confidence)
  4. Solver convergence (clean NR convergence = high, Brent fallback = suspect)
- Log all 4 confidence components alongside composite score for analysis
- Linear interpolation between observed IV points + flat extrapolation beyond boundaries (v1 -- simple, no negative vol, fast)
- Per-expiry smile only (no 2D strike+expiry surface). Call spread replication only needs same-expiry data
- Exclude strikes where IV bid-ask spread exceeds configurable threshold (hard filter). Linear interpolation doesn't support weighting -- garbage strikes kink the surface
- Log excluded strikes so data quality gaps are visible
- Configurable minimum usable strikes (default 3). Below minimum, fall back to ATM flat vol with confidence reflecting degradation
- Configurable "good" strike count (default 5) for confidence tiers
- Compute delta, vega, and theta per-instrument. Skip gamma for v1 (execution/hedging concern, irrelevant without hedging)
- Per-instrument only, not position-aggregated (aggregation is Phase 8's concern)
- Greeks attached to ImpliedProbability struct (single output, easy downstream consumption)
- Theta cross-checks the carry cost model from Phase 6 -- disagreement indicates a problem
- Underlying price: use specific futures contract matching the option's expiry (Black-76 is a futures model). Subscribe to relevant futures tickers. Fall back to index + carry estimate if specific futures illiquid, flag as lower confidence

### Claude's Discretion
- Exact Newton-Raphson convergence parameters (max iterations, tolerance)
- IV clamping bounds (min/max vol)
- Internal data structures for vol surface storage
- Exact confidence weight defaults
- Black-76 implementation details (d1/d2 computation, numerical stability)

### Deferred Ideas (OUT OF SCOPE)
- Gamma calculation -- add when execution/dynamic hedging is needed
- SABR or parametric vol surface model -- upgrade from linear interpolation in future iteration
- 2D vol surface (strike + expiry term structure) -- add if term structure analysis proves valuable
- Portfolio-level Greeks aggregation -- Phase 8 scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PRIC-01 | Implied volatility solver extracts IV from Deribit option mid-prices using Newton-Raphson or Brent's method with Black-76 model | Black-76 closed-form pricing (F, K, T, r, sigma -> price) with analytic vega for NR derivative; `statrs` 0.18.0 for Normal CDF; NR with Brent fallback pattern documented in Architecture section; Deribit `TickerData` provides `mark_price`, `bid_iv`, `ask_iv` for cross-validation |
| PRIC-02 | IV solver handles edge cases: deep ITM/OTM options, near-expiry theta collapse, negative time value | IV clamping with configurable bounds (min 1%, max 500%); near-expiry cutoff (default 2h) switches to intrinsic pricing; vega floor check prevents NR divergence; negative time value detection logs and flags |
| PRIC-03 | Probability extractor computes P(S > K) using multiple methods: naive N(d2), strike-specific vol N(d2), call spread replication, and full smile interpolation | N(d2) from `statrs::distribution::Normal::cdf(d2)`; call spread replication `(C(K-e) - C(K+e)) / (2*e)` using real adjacent Deribit strikes; vol surface provides strike-specific IV for N(d2) skew adjustment; all methods computed in parallel and logged |
| PRIC-04 | Call spread replication `(C(K-e) - C(K+e)) / 2e` is the primary digital pricing method, producing skew-adjusted probabilities | Uses nearest liquid adjacent strikes as epsilon (not arbitrary offset); configurable max epsilon; prices C(K-e) and C(K+e) using Black-76 with interpolated IV from vol surface; inherently skew-adjusted since it uses real market-consistent vol per strike |
| PRIC-05 | Implied volatility surface construction interpolates across strikes for pricing at non-traded strikes | Per-expiry vol smile: sorted (strike, IV) pairs, linear interpolation between observed points, flat extrapolation beyond boundaries; quality filters exclude strikes with IV bid-ask spread > threshold; minimum 3 strikes required, 5+ for "good" confidence |
| PRIC-06 | Each ImpliedProbability output includes: probability value, confidence (based on bid-ask width/depth), pricing method used, skew adjustment factor, and timestamp | `ImpliedProbability` struct with: probability, prob_bid, prob_ask, confidence (0.0-1.0 composite), confidence_components (4 individual scores), method enum, skew_adjustment, greeks, timestamp, solver metadata |
| PRIC-07 | Greeks calculator computes delta, gamma, vega, theta for position monitoring and downstream risk assessment | Black-76 analytic Greeks: delta = e^(-rT) * N(d1), vega = F * e^(-rT) * n(d1) * sqrt(T), theta (full formula). Skip gamma per user decision. Attached to ImpliedProbability output struct. Cross-validation against Deribit exchange-reported Greeks via `SnapshotGreeks` |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| statrs | 0.18.0 | Normal distribution CDF and PDF for Black-76 d1/d2 | Standard Rust statistics library; `Normal::standard().cdf(x)` for N(x); verified compiling on project's Rust 1.92.0 (MSRV 1.65); 14 transitive deps acceptable for a mature math library |
| rust_decimal | 1.40 | Price/Probability/Notional types for output | Already in Cargo.toml; final ImpliedProbability values use Decimal for consistency with downstream pipeline |
| serde / serde_json | 1.0 | Config deserialization, JSONL logging | Already in Cargo.toml |
| tokio | 1 | Async runtime, channels, timers | Already in Cargo.toml; pricing engine runs as async task consuming MarketSnapshots |
| metrics | 0.24 | Prometheus counters/histograms for pricing metrics | Already in Cargo.toml; recorder installed in Phase 6 |
| chrono | 0.4 | Expiry date parsing, time-to-maturity computation | Already in Cargo.toml |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| libm | (transitive) | Low-level math functions (erf, ln, exp, sqrt) | Already in dependency tree via other crates; available if needed for numerical edge cases |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| statrs for Normal CDF | Hand-roll using libm::erf | Fewer dependencies, but statrs is well-tested and battle-proven; use statrs |
| Hand-rolled NR+Brent IV solver | black76 crate (v0.24.2) | black76 uses f32 (not f64), depends on statrs 0.16 (outdated), and provides more than needed; hand-roll for precision control |
| Hand-rolled NR+Brent IV solver | black_scholes crate | Provides Black-76 mode but IV solver uses Black-Scholes (spot, not forward); hand-roll for exact Black-76 IV solving |
| Hand-rolled NR+Brent IV solver | roots crate (v0.0.8) for Brent | roots provides generic Brent's method, but last updated 2022, uses Rust 2015 edition; Brent's method is ~30 lines, hand-roll for zero dependency cost |
| statrs for Normal CDF | errorfunctions crate | More specialized but less ecosystem presence; statrs also provides PDF needed for vega computation |

**Dependencies to add:**
```toml
# In Cargo.toml [dependencies]
statrs = "0.18"
```

No other new external dependencies required. All other functionality (Black-76 pricer, IV solver, vol surface, probability extraction, Greeks) is hand-rolled.

## Architecture Patterns

### Recommended Project Structure
```
src/
├── pricing/
│   ├── mod.rs              # Module exports, PricingEngine orchestrator
│   ├── black76.rs          # Black-76 pricer: call_price, put_price, d1, d2, vega, delta, theta
│   ├── iv_solver.rs        # Newton-Raphson + Brent fallback IV solver
│   ├── vol_surface.rs      # Per-expiry vol smile (linear interpolation, quality filtering)
│   ├── probability.rs      # Probability extraction: call_spread_replication, n_d2, skew adjustment
│   ├── confidence.rs       # Confidence scorer (4-component weighted composite)
│   ├── greeks.rs           # Greeks computation (delta, vega, theta) using Black-76 analytics
│   ├── types.rs            # ImpliedProbability, PricingMethod, SolverResult, VolSmile, etc.
│   └── config.rs           # PricingConfig (TOML-driven parameters)
├── types/
│   └── snapshot.rs         # Extended: add bid_iv, ask_iv, underlying_price, underlying_index
└── ...existing modules...
```

### Pattern 1: Black-76 Pricing Core (f64 internal, Decimal output)
**What:** All internal pricing computation uses f64 (standard for numerical methods; no Decimal overhead). Final ImpliedProbability output converts to Decimal for pipeline consistency.
**When to use:** All pricing math -- d1/d2 computation, CDF evaluation, Greeks, IV solving.
**Example:**
```rust
use statrs::distribution::{ContinuousCDF, Normal};

/// Black-76 call price.
///
/// Parameters:
/// - f: forward/futures price
/// - k: strike price
/// - t: time to expiry in years
/// - sigma: implied volatility (annualized)
/// - r: risk-free rate (typically 0.0 for crypto futures)
fn black76_call(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    let sqrt_t = t.sqrt();
    let d1 = (f.ln() - k.ln() + 0.5 * sigma * sigma * t) / (sigma * sqrt_t);
    let d2 = d1 - sigma * sqrt_t;
    let norm = Normal::standard();
    let df = (-r * t).exp(); // discount factor
    df * (f * norm.cdf(d1) - k * norm.cdf(d2))
}

/// Black-76 vega (sensitivity to volatility).
/// Used as the derivative in Newton-Raphson IV solving.
fn black76_vega(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    let sqrt_t = t.sqrt();
    let d1 = (f.ln() - k.ln() + 0.5 * sigma * sigma * t) / (sigma * sqrt_t);
    let norm = Normal::standard();
    let df = (-r * t).exp();
    df * f * norm.pdf(d1) * sqrt_t  // n(d1) is the PDF
}
```

### Pattern 2: Newton-Raphson with Brent Fallback
**What:** Primary IV solver uses Newton-Raphson (quadratic convergence when vega is non-zero). Falls back to Brent's method (guaranteed convergence within bracket) when NR fails to converge in N iterations or produces out-of-bounds results.
**When to use:** Every IV solve call.
**Example:**
```rust
struct SolverResult {
    iv: f64,
    method: SolverMethod,     // NewtonRaphson or Brent
    iterations: u32,
    converged: bool,
    residual: f64,            // |model_price - market_price| at solution
}

enum SolverMethod {
    NewtonRaphson,
    Brent,
}

fn solve_iv(
    market_price: f64,
    f: f64, k: f64, t: f64, r: f64,
    is_call: bool,
    config: &SolverConfig,
) -> SolverResult {
    // 1. Initial guess: Brenner-Subrahmanyam approximation
    //    sigma_0 = sqrt(2*pi/T) * (C/F)  for ATM-ish options
    //    Or use exchange-provided mark_iv as seed if available
    let mut sigma = initial_guess(market_price, f, k, t);
    sigma = sigma.clamp(config.iv_min, config.iv_max);

    // 2. Newton-Raphson iterations
    for i in 0..config.nr_max_iterations {
        let model_price = black76_price(f, k, t, sigma, r, is_call);
        let vega = black76_vega(f, k, t, sigma, r);

        if vega.abs() < config.vega_floor {
            // Vega too small -- NR will diverge. Switch to Brent.
            break;
        }

        let diff = model_price - market_price;
        if diff.abs() < config.price_tolerance {
            return SolverResult {
                iv: sigma, method: SolverMethod::NewtonRaphson,
                iterations: i + 1, converged: true, residual: diff.abs(),
            };
        }

        sigma -= diff / vega;
        sigma = sigma.clamp(config.iv_min, config.iv_max);
    }

    // 3. Brent fallback (bracketed search)
    brent_solve(market_price, f, k, t, r, is_call, config)
}
```

### Pattern 3: Per-Expiry Vol Smile with Quality Filtering
**What:** For each expiry, collect (strike, mid_iv) pairs from all active options, filter out poor-quality strikes, sort by strike, store as sorted array for linear interpolation.
**When to use:** After IV solving, before probability extraction.
**Example:**
```rust
struct VolSmile {
    expiry: NaiveDate,
    /// Sorted by strike. Each entry is (strike, iv, quality metadata).
    points: Vec<SmilePoint>,
    /// Strikes excluded by quality filter (logged for visibility).
    excluded: Vec<(f64, String)>,  // (strike, reason)
    /// Quality tier: Good (5+ strikes), Minimum (3-4), Degraded (<3)
    quality: SmileQuality,
}

struct SmilePoint {
    strike: f64,
    iv: f64,
    bid_iv: f64,
    ask_iv: f64,
    iv_spread: f64,  // ask_iv - bid_iv
}

impl VolSmile {
    /// Interpolate IV at an arbitrary strike.
    /// Linear between observed points, flat extrapolation beyond boundaries.
    fn interpolate(&self, strike: f64) -> Option<f64> {
        if self.points.len() < 2 { return self.points.first().map(|p| p.iv); }

        // Below minimum strike: flat extrapolation
        if strike <= self.points.first().unwrap().strike {
            return Some(self.points.first().unwrap().iv);
        }
        // Above maximum strike: flat extrapolation
        if strike >= self.points.last().unwrap().strike {
            return Some(self.points.last().unwrap().iv);
        }

        // Binary search for surrounding points, linear interpolate
        // ...
    }
}
```

### Pattern 4: Call Spread Replication Using Real Adjacent Strikes
**What:** Digital option price P(S > K) is approximated by `(C(K - e) - C(K + e)) / (2 * e)` where e is the distance to the nearest liquid adjacent strikes, not an arbitrary offset.
**When to use:** Primary probability extraction method for every strike.
**Example:**
```rust
fn call_spread_probability(
    target_strike: f64,
    smile: &VolSmile,
    forward: f64,
    time_to_expiry: f64,
    rate: f64,
    config: &PricingConfig,
) -> Option<ProbabilityResult> {
    // Find nearest strikes below and above target in the smile
    let (k_lower, k_upper) = smile.nearest_bracket(target_strike)?;

    let epsilon = (k_upper - k_lower) / 2.0;
    if epsilon > config.max_epsilon {
        // Strikes too far apart -- fall back to N(d2)
        return None;
    }

    // Price calls at bracketing strikes using vol surface
    let iv_lower = smile.interpolate(k_lower)?;
    let iv_upper = smile.interpolate(k_upper)?;

    let c_lower = black76_call(forward, k_lower, time_to_expiry, iv_lower, rate);
    let c_upper = black76_call(forward, k_upper, time_to_expiry, iv_upper, rate);

    let prob = (c_lower - c_upper) / (k_upper - k_lower);

    // prob should be in [0, 1] for valid market data
    let prob = prob.clamp(0.0, 1.0);

    Some(ProbabilityResult {
        probability: prob,
        method: PricingMethod::CallSpreadReplication,
        epsilon_used: epsilon,
        k_lower,
        k_upper,
    })
}
```

### Pattern 5: Pricing Engine as Async Pipeline Stage
**What:** PricingEngine consumes Deribit MarketSnapshots, maintains per-expiry state (vol smiles, IV cache), and emits ImpliedProbability events downstream.
**When to use:** Main pipeline integration.
**Example:**
```rust
pub struct PricingEngine {
    /// Per-expiry vol smile state, keyed by expiry date string.
    smiles: HashMap<String, VolSmile>,
    /// Per-instrument IV cache (bid_iv, ask_iv, mid_iv).
    iv_cache: HashMap<String, IvTriple>,
    /// Configuration.
    config: PricingConfig,
}

impl PricingEngine {
    pub async fn run(
        mut self,
        mut snapshot_rx: mpsc::Receiver<MarketSnapshot>,
        probability_tx: mpsc::Sender<ImpliedProbability>,
        cancel: CancellationToken,
    ) {
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                snap = snapshot_rx.recv() => {
                    match snap {
                        Some(s) if s.venue == Venue::Deribit => {
                            self.process_deribit_snapshot(s, &probability_tx).await;
                        }
                        Some(_) => {} // non-Deribit snapshots pass through
                        None => break,
                    }
                }
            }
        }
    }
}
```

### Anti-Patterns to Avoid
- **Using Decimal for internal pricing math:** Decimal has no `ln()`, `exp()`, `sqrt()`, or CDF. Options math is inherently floating-point. Use f64 internally, convert to Decimal only at output boundaries.
- **Solving IV from scratch when exchange provides it:** Deribit provides `bid_iv`/`ask_iv`/`mark_iv` on every ticker update. Use exchange IV as initial guess to accelerate convergence, and as cross-validation after solving. Don't ignore free data.
- **Using perpetual price as forward:** Black-76 requires the forward/futures price matching the option's expiry. The perpetual-to-futures basis can be 1-3% in crypto, directly biasing IV and implied probability. Use the specific futures contract (`underlying_index` field from Deribit ticker, e.g., "BTC-27JUN25").
- **Flat vol assumption for call spread replication:** The whole point of call spread replication is to capture skew from real market prices. Each leg must use its own strike-specific IV from the vol surface, not ATM vol.
- **Dropping instruments with solver failures:** Never silently discard data. Clamp, flag, and pass through with degraded confidence. The interesting dislocations may be where the solver struggles.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Normal CDF/PDF | Custom Horner polynomial or Taylor series approximation | `statrs::distribution::Normal::standard().cdf(x)` / `.pdf(x)` | Battle-tested, accurate to machine precision, handles tails correctly |
| Error function (erf) | Custom approximation | `statrs` (internal) or `libm::erf` | Numerical edge cases in tails are subtle; use proven implementations |

**Key insight:** The Black-76 pricer, IV solver, vol surface, and probability extraction are all straightforward to implement with known formulas. The tricky part is edge case handling (numerical stability), not algorithmic complexity. Use `statrs` for the statistical building blocks; hand-roll the domain-specific assembly because no existing Rust crate provides exactly the Black-76 + call spread replication + confidence scoring pipeline needed.

## Common Pitfalls

### Pitfall 1: Near-Zero Vega Causes Newton-Raphson Divergence
**What goes wrong:** Deep OTM/ITM options have near-zero vega. NR update `sigma -= (model_price - market_price) / vega` produces enormous steps when vega is tiny, overshooting wildly.
**Why it happens:** Options far from the money have negligible sensitivity to vol changes.
**How to avoid:** Check `vega.abs() < vega_floor` before each NR step. If below threshold, immediately switch to Brent's method (bisection-like, guaranteed convergence). Typical vega_floor: 1e-10.
**Warning signs:** IV solver producing values > 500% or negative, or oscillating without convergence.

### Pitfall 2: Perpetual vs Futures Basis Corrupts IV
**What goes wrong:** Using BTC-PERPETUAL price (or spot index) instead of the specific futures contract as the underlying in Black-76 introduces systematic IV bias.
**Why it happens:** The basis between perpetual and a 3-month future can be 1-3% in crypto, directly shifting d1/d2 and thus IV.
**How to avoid:** Parse `underlying_index` from Deribit ticker data (e.g., "BTC-27JUN25") to identify the correct futures contract. Subscribe to that futures ticker for the forward price. Fall back to index + estimated carry only when specific futures data unavailable.
**Warning signs:** Computed IV systematically different from Deribit's `mark_iv` by 2-5% across all strikes for the same expiry.

### Pitfall 3: Time-to-Expiry Precision Near Zero
**What goes wrong:** When T approaches zero, `sigma * sqrt(T)` approaches zero, making d1 and d2 undefined (division by zero) or numerically unstable.
**Why it happens:** Options expiring within hours have T values like 0.0001 years.
**How to avoid:** Near-expiry cutoff (default 2 hours per user decision). Below cutoff, switch to intrinsic value: `max(0, F - K)` for calls, `max(0, K - F)` for puts. Flag clearly in output metadata.
**Warning signs:** NaN or Inf in d1/d2 calculations; probability outputs of exactly 0.0 or 1.0 for slightly OTM options near expiry.

### Pitfall 4: Negative Time Value in Market Prices
**What goes wrong:** Market mid-price is below intrinsic value (e.g., deep ITM option with wide bid-ask spread). IV solver has no solution because Black-76 always produces price >= intrinsic.
**Why it happens:** Wide bid-ask spreads in illiquid deep ITM options; mid-price is not a tradeable price.
**How to avoid:** Detect `market_price < intrinsic_value` before solving. If negative time value, report IV as 0.0 (or near-zero), flag with reason code "negative_time_value", set low confidence.
**Warning signs:** Brent's method returning `iv_min` bound; solver flagging non-convergence for deep ITM options.

### Pitfall 5: Vol Surface Quality Degrades Silently
**What goes wrong:** Far OTM/ITM options with wide bid-ask spreads produce noisy IV points that kink the linear interpolation surface, corrupting call spread replication at non-observed strikes.
**Why it happens:** Linear interpolation passes through every point -- one bad data point distorts the entire segment.
**How to avoid:** Hard filter: exclude strikes where `ask_iv - bid_iv > max_iv_spread_threshold` (configurable). Log excluded strikes for visibility. Require minimum 3 strikes after filtering; fall back to ATM flat vol if insufficient.
**Warning signs:** Interpolated IV producing non-monotonic call prices (arbitrage violation); call spread replication yielding negative probabilities.

### Pitfall 6: Deribit Options Are Inverse Contracts (BTC-denominated)
**What goes wrong:** Deribit BTC options are quoted in BTC (not USD). The "price" field in the order book is in BTC per 1 BTC notional. Directly using this price in Black-76 without understanding the inverse structure produces wrong IV.
**Why it happens:** Deribit uses Black-76 where the price is expressed as a fraction of the forward price (e.g., 0.0055 means the option costs 0.55% of the underlying).
**How to avoid:** Deribit option prices are already in the correct units for Black-76: the `mark_price` and order book prices are in BTC, the forward is in USD, and the formula accounts for this. The key is that Deribit's `mark_price` for options is the BTC-denominated price, and `underlying_price` is the USD-denominated forward. Follow Deribit's convention: `option_price_usd = mark_price * underlying_price`.
**Warning signs:** IV values off by orders of magnitude; computed prices not matching Deribit's `mark_price`.

## Code Examples

### Black-76 Full Pricing Suite
```rust
use statrs::distribution::{ContinuousCDF, Continuous, Normal};

const NORM: Normal = Normal::standard(); // compile-time? No -- use lazy_static or function-local

fn norm_cdf(x: f64) -> f64 {
    Normal::standard().cdf(x)
}

fn norm_pdf(x: f64) -> f64 {
    Normal::standard().pdf(x)
}

/// Compute d1 and d2 for Black-76.
fn d1_d2(f: f64, k: f64, t: f64, sigma: f64) -> (f64, f64) {
    let sqrt_t = t.sqrt();
    let d1 = ((f / k).ln() + 0.5 * sigma * sigma * t) / (sigma * sqrt_t);
    let d2 = d1 - sigma * sqrt_t;
    (d1, d2)
}

/// Black-76 call price. r is risk-free rate (typically ~0 for crypto).
fn call_price(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    let (d1, d2) = d1_d2(f, k, t, sigma);
    let df = (-r * t).exp();
    df * (f * norm_cdf(d1) - k * norm_cdf(d2))
}

/// Black-76 put price.
fn put_price(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    let (d1, d2) = d1_d2(f, k, t, sigma);
    let df = (-r * t).exp();
    df * (k * norm_cdf(-d2) - f * norm_cdf(-d1))
}

/// Black-76 vega (same for call and put).
fn vega(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    let (d1, _) = d1_d2(f, k, t, sigma);
    let df = (-r * t).exp();
    df * f * norm_pdf(d1) * t.sqrt()
}

/// Black-76 delta for call.
fn call_delta(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    let (d1, _) = d1_d2(f, k, t, sigma);
    (-r * t).exp() * norm_cdf(d1)
}

/// Black-76 theta for call (per year; divide by 365.25 for per-day).
fn call_theta(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    let (d1, d2) = d1_d2(f, k, t, sigma);
    let df = (-r * t).exp();
    let term1 = -(f * norm_pdf(d1) * sigma) / (2.0 * t.sqrt());
    let term2 = r * f * norm_cdf(d1);
    let term3 = -r * k * norm_cdf(d2);
    df * (term1 - term2 - term3)
}
```

### Brent's Method (Compact Implementation)
```rust
/// Brent's method for IV solving when Newton-Raphson fails.
/// Finds sigma such that black76_price(sigma) = market_price.
fn brent_iv(
    market_price: f64,
    f: f64, k: f64, t: f64, r: f64,
    is_call: bool,
    iv_min: f64, iv_max: f64,
    tolerance: f64,
    max_iter: u32,
) -> SolverResult {
    let price_fn = |sigma: f64| -> f64 {
        let p = if is_call { call_price(f, k, t, sigma, r) }
                else { put_price(f, k, t, sigma, r) };
        p - market_price
    };

    let mut a = iv_min;
    let mut b = iv_max;
    let mut fa = price_fn(a);
    let mut fb = price_fn(b);

    // Standard Brent's method implementation (~30 lines)
    // ...bisection with inverse quadratic interpolation...

    SolverResult {
        iv: best_sigma,
        method: SolverMethod::Brent,
        iterations: iter_count,
        converged: residual < tolerance,
        residual,
    }
}
```

### Confidence Scoring
```rust
/// Compute composite confidence score from 4 components.
fn compute_confidence(
    iv_bid_ask_spread: f64,  // e.g., ask_iv - bid_iv in vol points
    book_depth_usd: f64,     // total depth in USD at top N levels
    method_agreement: f64,   // |prob_callspread - prob_nd2|
    solver_quality: f64,     // 1.0 for clean NR, 0.5 for Brent, 0.0 for clamped
    weights: &ConfidenceWeights,
) -> (f64, ConfidenceComponents) {
    // Map each input to 0-1 score
    let iv_score = 1.0 - (iv_bid_ask_spread / weights.iv_spread_max).min(1.0);
    let depth_score = (book_depth_usd / weights.depth_target).min(1.0);
    let agreement_score = 1.0 - (method_agreement / weights.max_disagreement).min(1.0);
    let solver_score = solver_quality;

    let composite = weights.iv_weight * iv_score
        + weights.depth_weight * depth_score
        + weights.agreement_weight * agreement_score
        + weights.solver_weight * solver_score;

    let components = ConfidenceComponents {
        iv_spread: iv_score,
        book_depth: depth_score,
        method_agreement: agreement_score,
        solver_convergence: solver_score,
    };

    (composite.clamp(0.0, 1.0), components)
}
```

### N(d2) Probability with Skew Adjustment
```rust
/// Compute P(S > K) using N(d2) with optional skew adjustment.
///
/// Skew adjustment: use strike-specific IV (from vol surface) instead of ATM IV.
/// The skew magnitude (strike_iv - atm_iv) is logged as metadata.
fn nd2_probability(
    f: f64, k: f64, t: f64,
    strike_iv: f64,
    atm_iv: Option<f64>,
) -> ProbabilityResult {
    let (_, d2) = d1_d2(f, k, t, strike_iv);
    let prob = norm_cdf(d2);  // P(S > K) for calls under risk-neutral measure

    let skew_adjustment = atm_iv.map(|atm| strike_iv - atm).unwrap_or(0.0);

    ProbabilityResult {
        probability: prob.clamp(0.0, 1.0),
        method: PricingMethod::Nd2SkewAdjusted,
        skew_adjustment,
        // ...
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Naive N(d2) for digital probability | Call spread replication (skew-aware) | Standard in quant finance since ~2000s | N(d2) is biased under skew; call spread captures local skew from market prices |
| Brenner-Subrahmanyam initial guess | Jaeckel "Let's Be Rational" IV solver | 2017+ | Achieves machine-precision IV in 2 iterations; overkill for v1 but worth noting for future |
| Black-Scholes (spot model) | Black-76 (futures model) | Deribit uses futures settlement | Must use forward price, not spot, as underlying |
| SABR/SVI parametric smile | Linear interpolation (v1) | Project decision: simple first | Linear is adequate for adjacent-strike call spreads; SABR deferred |

**Deprecated/outdated:**
- Using `black76` crate v0.24.2: depends on `statrs 0.16` (outdated), uses `f32` precision, and `libc` C bindings. Not recommended for this project.
- Using spot price in Black-76: Deribit options are on futures. The `underlying_price` field from the ticker is the correct forward price.

## Data Flow: Existing Pipeline to Phase 7 Integration

### Key Finding: Missing Data in MarketSnapshot
The Deribit `TickerData` struct (messages.rs) already parses these fields from JSON:
- `bid_iv: Option<f64>` -- exchange-computed bid implied volatility
- `ask_iv: Option<f64>` -- exchange-computed ask implied volatility
- `underlying_price: Option<f64>` -- forward/futures price used by Deribit's pricer
- `underlying_index: Option<String>` -- futures contract name (e.g., "BTC-27JUN25")

However, **these fields are NOT carried through** to `TickerState` or `MarketSnapshot`. The normalization layer (normalize.rs) only preserves `mark_iv` and exchange Greeks.

**Phase 7 must extend** `TickerState` and `MarketSnapshot` to carry `bid_iv`, `ask_iv`, `underlying_price`, and `underlying_index`. This is a prerequisite for both IV cross-validation and correct forward price selection.

### Deribit Instrument Name Parsing
Deribit option instrument names follow the pattern: `{ASSET}-{DDMMMYY}-{STRIKE}-{TYPE}`
- Example: `BTC-27JUN25-100000-C`
- Asset: BTC
- Expiry: 27JUN25 (June 27, 2025)
- Strike: 100000 (USD)
- Type: C (Call) or P (Put)

Phase 7 needs a parser to extract strike, expiry, and option type from instrument names for:
1. Grouping options by expiry to build per-expiry vol smiles
2. Determining call vs put for IV solving
3. Extracting strike for vol surface coordinates

### Forward Price Selection
Black-76 requires the **futures price matching the option's expiry**, not the perpetual or spot index.

Deribit provides `underlying_index` (e.g., "BTC-27JUN25") on option tickers, identifying the specific futures contract. The system should:
1. Subscribe to futures ticker channels for all relevant expiries
2. Use the futures mark_price as the forward price F in Black-76
3. Fall back to `index_price` + estimated carry if futures data unavailable

## Configuration Pattern

```toml
[pricing]
# IV Solver
nr_max_iterations = 50
nr_price_tolerance = 1e-8
nr_vega_floor = 1e-10
iv_min = 0.01          # 1% annualized vol floor
iv_max = 5.0           # 500% annualized vol ceiling
brent_max_iterations = 100

# Near-expiry handling
near_expiry_cutoff_hours = 2.0

# Vol surface
min_usable_strikes = 3
good_strike_count = 5
max_iv_spread_filter = 0.50   # 50 vol points: exclude strikes with wider bid-ask IV spread

# Probability extraction
max_epsilon_usd = 10000.0     # Max call spread epsilon in USD

# Confidence weights (must sum to ~1.0)
confidence_iv_weight = 0.30
confidence_depth_weight = 0.20
confidence_agreement_weight = 0.30
confidence_solver_weight = 0.20

# Confidence scaling parameters
confidence_iv_spread_max = 20.0    # 20 vol points = 0 confidence on this component
confidence_depth_target = 100000.0  # $100K depth = full confidence on this component
confidence_max_disagreement = 0.10  # 10% probability disagreement = 0 confidence on this component

# Risk-free rate (typically ~0 for crypto; configurable for carry cost cross-check)
risk_free_rate = 0.0
```

## Open Questions

1. **Deribit option price units and Black-76 convention**
   - What we know: Deribit quotes options in BTC (inverse contract). The `mark_price` is in BTC. The `underlying_price` is in USD (forward price).
   - What's unclear: The exact unit conversion needed to input Deribit order book prices (in BTC) into Black-76. Deribit's own pricer handles this internally, but we need to replicate it. The formula is likely: `option_price_usd = option_price_btc * forward_price_usd`.
   - Recommendation: Verify against Deribit's `mark_iv` -- compute IV from mid-price and compare to exchange `mark_iv`. If they match (within tolerance), the unit conversion is correct. This should be the first validation test.

2. **Pipeline integration point**
   - What we know: Currently the pipeline is: feeds -> fan-in -> SpreadEngine -> PaperTradeTracker. Phase 7 adds a PricingEngine between the fan-in and SpreadEngine for Deribit data.
   - What's unclear: Whether PricingEngine should be inline (consuming Deribit snapshots, producing ImpliedProbability) or parallel (separate channel). Phase 8 will need both raw prediction market snapshots AND implied probabilities.
   - Recommendation: PricingEngine subscribes to the same fan-in channel, processes Deribit options snapshots, and publishes ImpliedProbability events on a separate channel. Phase 8 merges both channels. For v1, the PricingEngine can be a standalone task that logs probabilities without feeding into Phase 8 yet.

3. **Futures ticker subscription**
   - What we know: The Deribit supervisor currently subscribes to option tickers. Black-76 needs the futures price matching each option's expiry.
   - What's unclear: Whether the existing pipeline already subscribes to futures tickers, or if Phase 7 needs to add these subscriptions.
   - Recommendation: Check if `underlying_price` in option tickers is sufficient (it should be -- Deribit reports the forward price on option tickers). If so, no additional subscriptions needed. If not, add futures ticker subscriptions to the Deribit supervisor.

## Sources

### Primary (HIGH confidence)
- Deribit API documentation (https://docs.deribit.com/) -- ticker data format, settlement, inverse options
- statrs v0.18.0 docs (https://docs.rs/statrs/latest/statrs/) -- Normal CDF/PDF API, MSRV 1.65
- Black model Wikipedia (https://en.wikipedia.org/wiki/Black_model) -- Black-76 formulas, d1/d2
- LME Black-76 formula reference (https://www.lme.com/trading/contract-types/options/black-scholes-76-formula) -- official formula specification
- black_scholes crate docs (https://docs.rs/black_scholes/latest/black_scholes/) -- Black-76 function signatures, f64 API reference
- Deribit inverse options documentation (https://support.deribit.com/hc/en-us/articles/31424939096093-Inverse-Options) -- pricing in BTC, forward price usage

### Secondary (MEDIUM confidence)
- Quant Next: Binary Options Replication and Skew Sensitivity (https://quant-next.com/binary-options-pricing-replication-and-skew-sensitivity/) -- call spread replication theory
- Field Recordings: Digital options pricing by replication (https://fieldrecordings.wordpress.com/2011/01/07/digital-options-pricing-by-replication/) -- epsilon selection, skew correction
- InteractiveBrokers: Implied Volatility Robust Methods (https://www.interactivebrokers.com/campus/ibkr-quant-news/implied-volatility-formulation-computation-and-robust-numerical-methods/) -- NR edge cases, convergence
- roots crate docs (https://docs.rs/roots/latest/roots/) -- Brent's method reference (not recommended as dependency; hand-roll instead)

### Tertiary (LOW confidence)
- Medium articles on NR IV solving -- general patterns, unverified implementation details
- Various GitHub repos (hayden4r4/blackscholes-rust, etc.) -- reference implementations but not audited

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- statrs 0.18.0 verified compiling on Rust 1.92; Normal CDF API confirmed via docs.rs; no novel dependency risk
- Architecture: HIGH -- Black-76 is closed-form with known formulas; pipeline pattern matches existing codebase (async task consuming channel); all data types already exist in project
- Pitfalls: HIGH -- numerical edge cases well-documented in quant finance literature; Deribit-specific issues (inverse contracts, forward price) researched from official docs
- Implementation: MEDIUM -- Deribit option price unit conversion needs verification against exchange mark_iv; pipeline integration point (Phase 7 -> Phase 8 handoff) deferred to Phase 8 planning

**Research date:** 2026-02-23
**Valid until:** 2026-03-23 (30 days -- stable mathematical domain; Deribit API stable)
