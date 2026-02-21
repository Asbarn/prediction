use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Venue {
    Deribit,
    Polymarket,
    Kalshi,
}

impl fmt::Display for Venue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Venue::Deribit => write!(f, "deribit"),
            Venue::Polymarket => write!(f, "polymarket"),
            Venue::Kalshi => write!(f, "kalshi"),
        }
    }
}

impl Venue {
    /// Environment variable prefix for this venue's credentials.
    pub fn env_prefix(&self) -> &'static str {
        match self {
            Venue::Deribit => "DERIBIT",
            Venue::Polymarket => "POLYMARKET",
            Venue::Kalshi => "KALSHI",
        }
    }
}
