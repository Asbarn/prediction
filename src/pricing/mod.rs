//! Options pricing engine module.
//!
//! Provides Black-76 pricing, IV solving, vol surface interpolation,
//! probability extraction (call spread replication + N(d2)), confidence
//! scoring, and Greeks computation for Deribit options data.
//!
//! ## Sub-modules
//!
//! - `types` -- Core types: ImpliedProbability, SolverResult, PricingMethod, etc.
//! - `config` -- PricingConfig and sub-configs (TOML-driven parameters)
//! - `black76` -- Black-76 pricer: call/put price, vega, d1/d2
//! - `instrument` -- Deribit instrument name parser

pub mod black76;
pub mod confidence;
pub mod config;
pub mod greeks;
pub mod instrument;
pub mod iv_solver;
pub mod probability;
pub mod types;
pub mod vol_surface;
