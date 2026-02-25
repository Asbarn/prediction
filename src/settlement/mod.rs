//! Settlement outcome tracking module.
//!
//! Provides types, configuration, and venue-specific resolution checkers
//! for detecting how prediction market events and options expirations
//! resolved across Deribit, Kalshi, and Polymarket.

pub mod config;
pub mod deribit;
pub mod kalshi;
pub mod polymarket;
pub mod traits;
pub mod types;
