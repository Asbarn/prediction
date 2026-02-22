//! Polymarket reconnection supervisor.
//!
//! Long-lived task that wraps PolymarketClient with exponential backoff
//! reconnection, following the DeribitSupervisor pattern.
//!
//! Implemented in Task 2 (04-01-PLAN).
