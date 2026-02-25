//! Resolution checking trait and enum dispatch for venue-specific settlement logic.
//!
//! Uses enum dispatch (VenueChecker) instead of `dyn` trait objects to avoid
//! adding the `async-trait` crate dependency. Each venue's concrete checker type
//! is wrapped in the VenueChecker enum.

use rust_decimal::Decimal;

use crate::config::Direction;
use crate::types::Venue;

use super::types::ResolutionResult;

/// Context needed for resolution checks beyond the event_id and venue_instrument.
///
/// Carries expiry, asset, strike, and direction from the TrackedEvent/EventMapping
/// so that venue checkers can determine outcomes (e.g., Deribit needs strike + direction
/// to compare against delivery price).
#[derive(Debug, Clone)]
pub struct CheckContext {
    /// Expiry date string (e.g., "2025-06-27").
    pub expiry: String,
    /// Underlying asset (e.g., "BTC").
    pub asset: String,
    /// Strike price.
    pub strike: Decimal,
    /// Direction (above/below).
    pub direction: Direction,
}

/// Enum dispatch for venue-specific resolution checkers.
///
/// Wraps the three concrete checker types and delegates `check_resolution`
/// to the appropriate implementation. This avoids needing `async-trait` or
/// `dyn` trait objects.
pub enum VenueChecker {
    Deribit(super::deribit::DeribitResolutionChecker),
    Kalshi(super::kalshi::KalshiResolutionChecker),
    Polymarket(super::polymarket::PolymarketResolutionChecker),
}

impl VenueChecker {
    /// Check the resolution status of an event on this venue.
    pub async fn check_resolution(
        &self,
        event_id: &str,
        venue_instrument: &str,
        context: &CheckContext,
    ) -> anyhow::Result<ResolutionResult> {
        match self {
            VenueChecker::Deribit(checker) => {
                checker.check_resolution(event_id, venue_instrument, context).await
            }
            VenueChecker::Kalshi(checker) => {
                checker.check_resolution(event_id, venue_instrument, context).await
            }
            VenueChecker::Polymarket(checker) => {
                checker.check_resolution(event_id, venue_instrument, context).await
            }
        }
    }

    /// Which venue this checker handles.
    pub fn venue(&self) -> Venue {
        match self {
            VenueChecker::Deribit(_) => Venue::Deribit,
            VenueChecker::Kalshi(_) => Venue::Kalshi,
            VenueChecker::Polymarket(_) => Venue::Polymarket,
        }
    }
}
