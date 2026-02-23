use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Dual timestamp capturing both monotonic and wall-clock time.
///
/// - `mono`: monotonic instant for latency measurement and staleness checks.
///   Not serializable -- only meaningful within this process.
/// - `wall`: wall-clock time for logging, display, and serialization.
#[derive(Debug, Clone, Copy)]
pub struct DualTimestamp {
    pub mono: tokio::time::Instant,
    pub wall: DateTime<Utc>,
}

impl DualTimestamp {
    /// Capture both clocks simultaneously.
    pub fn now() -> Self {
        Self {
            mono: tokio::time::Instant::now(),
            wall: Utc::now(),
        }
    }

    /// Duration elapsed since this timestamp was captured (monotonic).
    pub fn elapsed(&self) -> std::time::Duration {
        self.mono.elapsed()
    }

    /// Wall-clock time getter.
    pub fn wall(&self) -> DateTime<Utc> {
        self.wall
    }
}

impl Serialize for DualTimestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.wall.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DualTimestamp {
    /// Deserialize from wall-clock time only.
    ///
    /// The monotonic instant is set to `Instant::now()` since it has no
    /// meaningful value when reconstructed from serialized data.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wall = DateTime::<Utc>::deserialize(deserializer)?;
        Ok(Self {
            mono: tokio::time::Instant::now(),
            wall,
        })
    }
}
