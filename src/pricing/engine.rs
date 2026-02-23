//! PricingEngine: async pipeline stage that consumes Deribit MarketSnapshots,
//! solves implied volatility, constructs per-expiry vol surfaces, extracts
//! probabilities, computes Greeks, scores confidence, and emits
//! ImpliedProbability events for downstream consumption (Phase 8).

use std::collections::HashMap;

use chrono::{NaiveDate, Utc};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::pricing::confidence::{compute_confidence, solver_quality_score};
use crate::pricing::config::PricingConfig;
use crate::pricing::greeks::compute_greeks;
use crate::pricing::instrument::parse_deribit_instrument;
use crate::pricing::iv_solver::solve_iv_triple;
use crate::pricing::probability::extract_probabilities;
use crate::pricing::types::{
    ConfidenceComponents, ImpliedProbability, InstrumentGreeks, OptionType, PricingMethod,
    SolverResult,
};
use crate::pricing::vol_surface::{SmilePoint, VolSmile};
use crate::types::{DualTimestamp, InstrumentId, MarketSnapshot, Probability, Venue};

// ---------------------------------------------------------------------------
// Per-instrument IV cache entry
// ---------------------------------------------------------------------------

/// Cached IV solve results for a single instrument.
struct IvCacheEntry {
    bid_iv: f64,
    ask_iv: f64,
    mid_iv: f64,
    bid_solver: SolverResult,
    #[allow(dead_code)]
    ask_solver: SolverResult,
    mid_solver: SolverResult,
    strike: f64,
    is_call: bool,
    forward: f64,
    time_to_expiry: f64,
}

// ---------------------------------------------------------------------------
// PricingEngine
// ---------------------------------------------------------------------------

/// Async pipeline stage orchestrating the full options pricing pipeline:
/// IV solving -> vol surface -> probability extraction -> Greeks -> confidence -> output.
pub struct PricingEngine {
    config: PricingConfig,
    /// Per-expiry vol smile, rebuilt on each update.
    smiles: HashMap<NaiveDate, VolSmile>,
    /// Per-instrument latest IV solve results.
    iv_cache: HashMap<InstrumentId, IvCacheEntry>,
    /// Raw IV data accumulated per-expiry for smile rebuilds.
    smile_points: HashMap<NaiveDate, HashMap<u64, SmilePoint>>,
    /// Stats counters for periodic logging.
    total_computed: u64,
    total_brent_fallbacks: u64,
    total_confidence_sum: f64,
}

/// Convert strike f64 to a u64 key for the smile_points map.
/// Strikes are always positive integers or simple decimals on Deribit.
fn strike_key(strike: f64) -> u64 {
    (strike * 100.0) as u64
}

impl PricingEngine {
    /// Create a new PricingEngine with the given configuration.
    pub fn new(config: PricingConfig) -> Self {
        Self {
            config,
            smiles: HashMap::new(),
            iv_cache: HashMap::new(),
            smile_points: HashMap::new(),
            total_computed: 0,
            total_brent_fallbacks: 0,
            total_confidence_sum: 0.0,
        }
    }

    /// Run the pricing engine, consuming snapshots and emitting ImpliedProbability events.
    ///
    /// Processes each Deribit option snapshot through:
    /// 1. Instrument parsing (skip non-options)
    /// 2. IV solving (bid/ask/mid)
    /// 3. Vol surface update
    /// 4. Probability extraction (call spread + N(d2))
    /// 5. Greeks computation
    /// 6. Confidence scoring
    /// 7. ImpliedProbability emission
    pub async fn run(
        mut self,
        mut snapshot_rx: mpsc::Receiver<MarketSnapshot>,
        probability_tx: mpsc::Sender<ImpliedProbability>,
        cancel: CancellationToken,
    ) {
        info!(
            near_expiry_cutoff_hours = self.config.near_expiry_cutoff_hours,
            iv_min = self.config.solver.iv_min,
            iv_max = self.config.solver.iv_max,
            "pricing engine started"
        );

        loop {
            tokio::select! {
                biased;

                _ = cancel.cancelled() => {
                    info!(
                        total_computed = self.total_computed,
                        active_expiries = self.smiles.len(),
                        "pricing engine shutting down"
                    );
                    break;
                }

                snapshot = snapshot_rx.recv() => {
                    let Some(snapshot) = snapshot else {
                        info!("pricing engine: snapshot channel closed");
                        break;
                    };

                    // Only process Deribit snapshots (options venue)
                    if snapshot.venue != Venue::Deribit {
                        continue;
                    }

                    self.process_snapshot(snapshot, &probability_tx).await;
                }
            }
        }
    }

    async fn process_snapshot(
        &mut self,
        snapshot: MarketSnapshot,
        probability_tx: &mpsc::Sender<ImpliedProbability>,
    ) {
        // a. Parse instrument name -- skip non-options (futures, perpetuals)
        let instrument_name = snapshot.instrument_id.to_string();
        let parsed = match parse_deribit_instrument(&instrument_name) {
            Some(p) => p,
            None => return, // Not an option -- skip silently
        };

        let is_call = parsed.option_type == OptionType::Call;
        let strike = parsed.strike;
        let expiry = parsed.expiry;

        // b. Extract forward price
        let forward = match snapshot.underlying_price {
            Some(f) if f > 0.0 => f,
            _ => {
                warn!(
                    instrument = %instrument_name,
                    "no underlying_price available, skipping"
                );
                return;
            }
        };

        // c. Compute time to expiry in years
        let now = Utc::now().naive_utc().date();
        let days_to_expiry = (expiry - now).num_days();
        let time_to_expiry = days_to_expiry as f64 / 365.25;

        let near_expiry_cutoff_years =
            self.config.near_expiry_cutoff_hours / (365.25 * 24.0);

        if time_to_expiry < near_expiry_cutoff_years {
            // Near-expiry intrinsic pricing path
            self.process_near_expiry(
                &snapshot,
                is_call,
                strike,
                forward,
                time_to_expiry,
                probability_tx,
            )
            .await;
            return;
        }

        // d. Compute market prices (bid, ask, mid)
        let bid_price_btc = snapshot.bid.map(|p| {
            let d: Decimal = *p;
            d.to_f64().unwrap_or(0.0)
        });
        let ask_price_btc = snapshot.ask.map(|p| {
            let d: Decimal = *p;
            d.to_f64().unwrap_or(0.0)
        });

        let (bid_price_btc, ask_price_btc) = match (bid_price_btc, ask_price_btc) {
            (Some(b), Some(a)) if b > 0.0 && a > 0.0 => (b, a),
            _ => {
                debug!(
                    instrument = %instrument_name,
                    "missing bid/ask, skipping IV solve"
                );
                return;
            }
        };

        let mid_price_btc = (bid_price_btc + ask_price_btc) / 2.0;

        // e. Deribit inverse option convention: price_usd = price_btc * forward
        let bid_price_usd = bid_price_btc * forward;
        let ask_price_usd = ask_price_btc * forward;
        let mid_price_usd = mid_price_btc * forward;

        let rate = self.config.risk_free_rate;

        // f. Solve IV triple
        let (bid_result, ask_result, mid_result) = solve_iv_triple(
            bid_price_usd,
            ask_price_usd,
            mid_price_usd,
            forward,
            strike,
            time_to_expiry,
            rate,
            is_call,
            &self.config.solver,
        );

        // Track Brent fallbacks
        if bid_result.method == crate::pricing::types::SolverMethod::Brent {
            self.total_brent_fallbacks += 1;
        }
        if ask_result.method == crate::pricing::types::SolverMethod::Brent {
            self.total_brent_fallbacks += 1;
        }
        if mid_result.method == crate::pricing::types::SolverMethod::Brent {
            self.total_brent_fallbacks += 1;
        }

        let mid_iv = mid_result.iv;
        let bid_iv = bid_result.iv;
        let ask_iv = ask_result.iv;

        // g. Update IV cache
        self.iv_cache.insert(
            snapshot.instrument_id.clone(),
            IvCacheEntry {
                bid_iv,
                ask_iv,
                mid_iv,
                bid_solver: bid_result.clone(),
                ask_solver: ask_result,
                mid_solver: mid_result.clone(),
                strike,
                is_call,
                forward,
                time_to_expiry,
            },
        );

        // h. Update smile_points for this expiry
        let iv_spread = ask_iv - bid_iv;
        let point = SmilePoint {
            strike,
            iv: mid_iv,
            bid_iv,
            ask_iv,
            iv_spread: iv_spread.max(0.0),
        };

        let expiry_points = self.smile_points.entry(expiry).or_default();
        expiry_points.insert(strike_key(strike), point);

        // i. Rebuild VolSmile for this expiry
        let raw_points: Vec<SmilePoint> = expiry_points.values().cloned().collect();
        let smile = VolSmile::new(
            expiry,
            raw_points,
            &self.config.vol_surface,
            forward,
        );
        self.smiles.insert(expiry, smile);

        let smile = self.smiles.get(&expiry).unwrap();

        // j. Extract probabilities
        let prob_extraction = extract_probabilities(
            strike,
            smile,
            forward,
            time_to_expiry,
            rate,
            &self.config,
        );

        let (primary_probability, method, method_disagreement, skew_adjustment, epsilon_used) =
            match prob_extraction {
                Some(pe) => {
                    let method = match pe.primary_method {
                        crate::pricing::probability::ProbabilityMethod::CallSpreadReplication => {
                            PricingMethod::CallSpreadReplication
                        }
                        crate::pricing::probability::ProbabilityMethod::Nd2SkewAdjusted => {
                            PricingMethod::Nd2SkewAdjusted
                        }
                    };
                    let epsilon = pe.call_spread.as_ref().map(|cs| cs.epsilon_used);
                    (
                        pe.primary_probability,
                        method,
                        pe.method_disagreement,
                        pe.skew_adjustment,
                        epsilon,
                    )
                }
                None => {
                    // Fallback: use N(d2) directly from Black-76
                    let norm = statrs::distribution::Normal::standard();
                    use statrs::distribution::ContinuousCDF;
                    let (_, d2) =
                        crate::pricing::black76::d1_d2(forward, strike, time_to_expiry, mid_iv);
                    let prob = norm.cdf(d2);
                    (prob, PricingMethod::Nd2SkewAdjusted, 0.0, 0.0, None)
                }
            };

        // k. Compute Greeks
        let greeks = compute_greeks(forward, strike, time_to_expiry, mid_iv, rate, is_call);

        // l. Compute book depth USD
        let book_depth_usd = compute_book_depth(&snapshot, forward);

        // m. Compute confidence
        let solver_quality = solver_quality_score(&mid_result);
        let (confidence, confidence_components) = compute_confidence(
            iv_spread.abs(),
            book_depth_usd,
            method_disagreement,
            solver_quality,
            &self.config.confidence,
        );

        // n. Assemble ImpliedProbability
        let probability = clamp_to_probability(primary_probability);
        let prob_bid = clamp_to_probability_opt(Some(bid_iv_to_prob(
            forward,
            strike,
            time_to_expiry,
            bid_iv,
        )));
        let prob_ask = clamp_to_probability_opt(Some(ask_iv_to_prob(
            forward,
            strike,
            time_to_expiry,
            ask_iv,
        )));

        let implied_prob = ImpliedProbability {
            instrument_id: snapshot.instrument_id.clone(),
            probability,
            prob_bid,
            prob_ask,
            confidence,
            confidence_components,
            method,
            skew_adjustment,
            greeks,
            solver_meta: Some(mid_result),
            epsilon_used,
            underlying_price: forward,
            timestamp: DualTimestamp::now(),
            near_expiry: false,
        };

        // o. Send via try_send (non-blocking)
        if let Err(e) = probability_tx.try_send(implied_prob) {
            debug!(
                instrument = %instrument_name,
                error = %e,
                "probability channel full or closed, dropping event"
            );
        }

        // Update stats
        self.total_computed += 1;
        self.total_confidence_sum += confidence;

        // Debug logging
        debug!(
            instrument = %instrument_name,
            mid_iv = format!("{:.4}", mid_iv),
            bid_iv = format!("{:.4}", bid_iv),
            ask_iv = format!("{:.4}", ask_iv),
            probability = format!("{:.4}", primary_probability),
            confidence = format!("{:.3}", confidence),
            method = ?method,
            skew = format!("{:.4}", skew_adjustment),
            "pricing computed"
        );

        // Periodic info logging (every 100 computations)
        if self.total_computed % 100 == 0 {
            let mean_confidence = self.total_confidence_sum / self.total_computed as f64;
            let total_solves = self.total_computed * 3; // bid + ask + mid
            let brent_rate = if total_solves > 0 {
                self.total_brent_fallbacks as f64 / total_solves as f64
            } else {
                0.0
            };
            info!(
                total_computed = self.total_computed,
                mean_confidence = format!("{:.3}", mean_confidence),
                brent_fallback_rate = format!("{:.3}", brent_rate),
                active_expiries = self.smiles.len(),
                "pricing engine stats"
            );
        }

        // Prometheus metrics
        metrics::counter!("pricing_iv_solves_total").increment(3); // bid + ask + mid
        metrics::histogram!("pricing_confidence").record(confidence);
        metrics::gauge!("pricing_active_expiries").set(self.smiles.len() as f64);
    }

    /// Process a near-expiry option using intrinsic pricing.
    async fn process_near_expiry(
        &mut self,
        snapshot: &MarketSnapshot,
        is_call: bool,
        strike: f64,
        forward: f64,
        _time_to_expiry: f64,
        probability_tx: &mpsc::Sender<ImpliedProbability>,
    ) {
        let probability_f64 = if is_call {
            if forward > strike {
                1.0
            } else {
                0.0
            }
        } else if strike > forward {
            1.0
        } else {
            0.0
        };

        let delta = if is_call {
            if forward > strike {
                1.0
            } else {
                0.0
            }
        } else if strike > forward {
            -1.0
        } else {
            0.0
        };

        let probability = clamp_to_probability(probability_f64);

        let implied_prob = ImpliedProbability {
            instrument_id: snapshot.instrument_id.clone(),
            probability,
            prob_bid: None,
            prob_ask: None,
            confidence: 0.3,
            confidence_components: ConfidenceComponents {
                iv_spread: 0.0,
                book_depth: 0.0,
                method_agreement: 0.0,
                solver_convergence: 0.0,
            },
            method: PricingMethod::IntrinsicOnly,
            skew_adjustment: 0.0,
            greeks: InstrumentGreeks {
                delta,
                vega: 0.0,
                theta: 0.0,
            },
            solver_meta: None,
            epsilon_used: None,
            underlying_price: forward,
            timestamp: DualTimestamp::now(),
            near_expiry: true,
        };

        if let Err(e) = probability_tx.try_send(implied_prob) {
            debug!(
                instrument = %snapshot.instrument_id,
                error = %e,
                "probability channel full or closed (near-expiry)"
            );
        }

        self.total_computed += 1;

        debug!(
            instrument = %snapshot.instrument_id,
            probability = probability_f64,
            method = "IntrinsicOnly",
            near_expiry = true,
            "near-expiry intrinsic pricing"
        );
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Compute total book depth in USD from snapshot depth levels.
fn compute_book_depth(snapshot: &MarketSnapshot, forward: f64) -> f64 {
    let bid_depth: f64 = snapshot
        .depth_bids
        .iter()
        .take(5)
        .map(|(_price, size)| {
            let d: Decimal = **size;
            d.to_f64().unwrap_or(0.0) * forward
        })
        .sum();

    let ask_depth: f64 = snapshot
        .depth_asks
        .iter()
        .take(5)
        .map(|(_price, size)| {
            let d: Decimal = **size;
            d.to_f64().unwrap_or(0.0) * forward
        })
        .sum();

    bid_depth + ask_depth
}

/// Convert an IV to a probability via N(d2) for bid/ask probability bounds.
fn bid_ask_iv_to_prob(forward: f64, strike: f64, t: f64, iv: f64) -> f64 {
    if iv <= 0.0 || t <= 0.0 {
        return if forward > strike { 1.0 } else { 0.0 };
    }
    let (_, d2) = crate::pricing::black76::d1_d2(forward, strike, t, iv);
    let norm = statrs::distribution::Normal::standard();
    use statrs::distribution::ContinuousCDF;
    norm.cdf(d2)
}

fn bid_iv_to_prob(forward: f64, strike: f64, t: f64, iv: f64) -> f64 {
    bid_ask_iv_to_prob(forward, strike, t, iv)
}

fn ask_iv_to_prob(forward: f64, strike: f64, t: f64, iv: f64) -> f64 {
    bid_ask_iv_to_prob(forward, strike, t, iv)
}

/// Clamp f64 probability to [0, 1] and convert to Probability type.
fn clamp_to_probability(p: f64) -> Probability {
    let clamped = p.clamp(0.0, 1.0);
    let decimal = Decimal::from_f64(clamped).unwrap_or(Decimal::ZERO);
    // Clamp Decimal to [0, 1] for safety
    let decimal = decimal.max(Decimal::ZERO).min(Decimal::ONE);
    Probability::new(decimal).unwrap_or_else(|_| Probability::new(Decimal::ZERO).unwrap())
}

fn clamp_to_probability_opt(p: Option<f64>) -> Option<Probability> {
    p.map(|v| clamp_to_probability(v))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::config::PricingConfig;
    use crate::types::{DualTimestamp, InstrumentId, Price, TraceId, Venue};
    use tokio::sync::mpsc;

    /// Build a mock Deribit option snapshot for testing.
    fn mock_deribit_option_snapshot(
        instrument: &str,
        bid: f64,
        ask: f64,
        underlying: f64,
    ) -> MarketSnapshot {
        MarketSnapshot {
            venue: Venue::Deribit,
            instrument_id: InstrumentId::new(instrument),
            event_id: None,
            bid: Some(Price::new(Decimal::from_f64(bid).unwrap())),
            ask: Some(Price::new(Decimal::from_f64(ask).unwrap())),
            bid_size: None,
            ask_size: None,
            depth_bids: vec![],
            depth_asks: vec![],
            bid_probability: None,
            ask_probability: None,
            last_price: None,
            mark_price: None,
            index_price: None,
            mark_iv: None,
            open_interest: None,
            volume_24h: None,
            greeks: None,
            bid_iv: None,
            ask_iv: None,
            underlying_price: Some(underlying),
            underlying_index: None,
            exchange_timestamp: None,
            timestamp: DualTimestamp::now(),
            sequence: 1,
            trace_id: TraceId::new(),
            is_stale: false,
        }
    }

    /// Test a: Processing a Deribit option snapshot produces an ImpliedProbability.
    #[tokio::test]
    async fn processes_option_snapshot() {
        let config = PricingConfig::default();
        let engine = PricingEngine::new(config);

        let (snap_tx, snap_rx) = mpsc::channel::<MarketSnapshot>(16);
        let (prob_tx, mut prob_rx) = mpsc::channel::<ImpliedProbability>(16);
        let cancel = CancellationToken::new();

        let cancel_clone = cancel.clone();
        let handle = tokio::spawn(engine.run(snap_rx, prob_tx, cancel_clone));

        // Send a mock Deribit option snapshot (far-future expiry to avoid near-expiry path)
        // Bid=0.08 BTC, Ask=0.10 BTC with underlying $100000
        let snapshot =
            mock_deribit_option_snapshot("BTC-27JUN27-100000-C", 0.08, 0.10, 100000.0);
        snap_tx.send(snapshot).await.unwrap();

        // Wait for the probability output
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), prob_rx.recv())
            .await
            .expect("timeout waiting for probability")
            .expect("channel closed");

        assert_eq!(result.instrument_id.to_string(), "BTC-27JUN27-100000-C");
        assert!(!result.near_expiry);
        assert!(result.confidence > 0.0);
        assert!(result.confidence <= 1.0);
        assert!(result.underlying_price > 0.0);

        cancel.cancel();
        let _ = handle.await;
    }

    /// Test b: Non-option instruments (futures) are skipped.
    #[tokio::test]
    async fn skips_non_option_instruments() {
        let config = PricingConfig::default();
        let engine = PricingEngine::new(config);

        let (snap_tx, snap_rx) = mpsc::channel::<MarketSnapshot>(16);
        let (prob_tx, mut prob_rx) = mpsc::channel::<ImpliedProbability>(16);
        let cancel = CancellationToken::new();

        let cancel_clone = cancel.clone();
        let handle = tokio::spawn(engine.run(snap_rx, prob_tx, cancel_clone));

        // Send a futures instrument (not an option -- 3 parts, not 4)
        let snapshot = mock_deribit_option_snapshot("BTC-27JUN27", 0.08, 0.10, 100000.0);
        snap_tx.send(snapshot).await.unwrap();

        // Send an option after the futures to verify the engine is still running
        let snapshot2 =
            mock_deribit_option_snapshot("BTC-27JUN27-100000-C", 0.08, 0.10, 100000.0);
        snap_tx.send(snapshot2).await.unwrap();

        // We should receive exactly one probability (from the option, not the future)
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), prob_rx.recv())
            .await
            .expect("timeout waiting for probability")
            .expect("channel closed");

        assert_eq!(result.instrument_id.to_string(), "BTC-27JUN27-100000-C");

        cancel.cancel();
        let _ = handle.await;
    }

    /// Test c: Near-expiry options produce IntrinsicOnly method with near_expiry=true.
    #[tokio::test]
    async fn near_expiry_produces_intrinsic() {
        let config = PricingConfig {
            near_expiry_cutoff_hours: 50000.0, // ~5.7 years -- force near-expiry for test
            ..PricingConfig::default()
        };
        let engine = PricingEngine::new(config);

        let (snap_tx, snap_rx) = mpsc::channel::<MarketSnapshot>(16);
        let (prob_tx, mut prob_rx) = mpsc::channel::<ImpliedProbability>(16);
        let cancel = CancellationToken::new();

        let cancel_clone = cancel.clone();
        let handle = tokio::spawn(engine.run(snap_rx, prob_tx, cancel_clone));

        // Send an option that will be within near-expiry window
        // Use far-future expiry but near_expiry_cutoff_hours is set to 2400h (100 days)
        let snapshot =
            mock_deribit_option_snapshot("BTC-27JUN27-90000-C", 0.08, 0.10, 100000.0);
        snap_tx.send(snapshot).await.unwrap();

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), prob_rx.recv())
            .await
            .expect("timeout waiting for probability")
            .expect("channel closed");

        assert!(result.near_expiry, "should be flagged as near-expiry");
        assert_eq!(result.method, PricingMethod::IntrinsicOnly);
        assert!(
            (result.confidence - 0.3).abs() < f64::EPSILON,
            "near-expiry confidence should be 0.3, got {}",
            result.confidence
        );

        // For this call with forward > strike (100000 > 90000), probability should be 1.0
        let prob_val = result.probability.into_inner();
        assert_eq!(prob_val, Decimal::ONE, "ITM call near-expiry prob should be 1.0");

        // Greeks: intrinsic delta for ITM call = 1.0
        assert!(
            (result.greeks.delta - 1.0).abs() < f64::EPSILON,
            "near-expiry ITM call delta should be 1.0, got {}",
            result.greeks.delta
        );
        assert!(
            result.greeks.vega.abs() < f64::EPSILON,
            "near-expiry vega should be 0"
        );

        cancel.cancel();
        let _ = handle.await;
    }

    /// Test d: Non-Deribit venues are ignored.
    #[tokio::test]
    async fn ignores_non_deribit_venues() {
        let config = PricingConfig::default();
        let engine = PricingEngine::new(config);

        let (snap_tx, snap_rx) = mpsc::channel::<MarketSnapshot>(16);
        let (prob_tx, mut prob_rx) = mpsc::channel::<ImpliedProbability>(16);
        let cancel = CancellationToken::new();

        let cancel_clone = cancel.clone();
        let handle = tokio::spawn(engine.run(snap_rx, prob_tx, cancel_clone));

        // Send a Polymarket snapshot (should be ignored by PricingEngine)
        let mut snapshot =
            mock_deribit_option_snapshot("BTC-27JUN27-100000-C", 0.08, 0.10, 100000.0);
        snapshot.venue = Venue::Polymarket;
        snap_tx.send(snapshot).await.unwrap();

        // Should not produce any output
        let result = tokio::time::timeout(std::time::Duration::from_millis(200), prob_rx.recv())
            .await;

        assert!(result.is_err(), "should timeout -- no output for non-Deribit");

        cancel.cancel();
        let _ = handle.await;
    }
}
