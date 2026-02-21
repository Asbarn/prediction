use derive_more::{Add, Deref, Display, From, Sub};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A price value. Wraps `Decimal` for type safety -- prevents accidental mixing
/// with `Probability` or `Notional` at compile time.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
    Add, Sub, From, Display, Deref,
    Serialize, Deserialize,
)]
#[display("{_0}")]
pub struct Price(
    #[serde(with = "rust_decimal::serde::str")]
    Decimal,
);

impl Price {
    pub fn new(value: Decimal) -> Self {
        Self(value)
    }

    pub fn into_inner(self) -> Decimal {
        self.0
    }
}

/// A probability value in [0, 1]. Construction validates the range.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
    Add, Sub, From, Display, Deref,
    Serialize, Deserialize,
)]
#[display("{_0}")]
pub struct Probability(
    #[serde(with = "rust_decimal::serde::str")]
    Decimal,
);

impl Probability {
    /// Construct with validation: must be in [0, 1].
    pub fn new(value: Decimal) -> Result<Self, &'static str> {
        if value < Decimal::ZERO || value > Decimal::ONE {
            return Err("probability must be between 0 and 1");
        }
        Ok(Self(value))
    }

    /// Complement: 1 - p.
    pub fn complement(&self) -> Self {
        Self(Decimal::ONE - self.0)
    }

    pub fn into_inner(self) -> Decimal {
        self.0
    }
}

/// A notional (size/quantity) value. Wraps `Decimal` for type safety.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
    Add, Sub, From, Display, Deref,
    Serialize, Deserialize,
)]
#[display("{_0}")]
pub struct Notional(
    #[serde(with = "rust_decimal::serde::str")]
    Decimal,
);

impl Notional {
    pub fn new(value: Decimal) -> Self {
        Self(value)
    }

    pub fn into_inner(self) -> Decimal {
        self.0
    }
}

/// Notional * Probability = Notional (expected value scaling).
impl std::ops::Mul<Probability> for Notional {
    type Output = Notional;

    fn mul(self, rhs: Probability) -> Self::Output {
        Notional(self.0 * rhs.0)
    }
}

/// Notional * Price = Notional (position sizing).
impl std::ops::Mul<Price> for Notional {
    type Output = Notional;

    fn mul(self, rhs: Price) -> Self::Output {
        Notional(self.0 * rhs.0)
    }
}
