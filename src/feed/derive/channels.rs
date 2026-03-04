//! Derive channel name construction and channel-type routing.
//!
//! Derive subscription channels follow patterns like:
//! - `orderbook.{instrument}.{group}.{depth}`
//! - `ticker_slim.{instrument}.{interval_ms}`

/// The kind of a Derive subscription channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeriveChannelKind {
    /// Order book snapshots: `orderbook.{instrument}.{group}.{depth}`
    Orderbook,
    /// Ticker slim data: `ticker_slim.{instrument}.{interval_ms}`
    TickerSlim,
    /// Unknown channel type.
    Unknown(String),
}

impl DeriveChannelKind {
    /// Parse a channel name string into a `DeriveChannelKind`.
    pub fn parse(channel: &str) -> Self {
        if channel.starts_with("orderbook.") {
            DeriveChannelKind::Orderbook
        } else if channel.starts_with("ticker_slim.") {
            DeriveChannelKind::TickerSlim
        } else {
            DeriveChannelKind::Unknown(channel.to_string())
        }
    }
}

/// Build the full set of subscription channel names for a list of instruments.
///
/// For each instrument, produces:
/// - `orderbook.{instrument}.10.{book_depth}` (group=10, depth=book_depth)
/// - `ticker_slim.{instrument}.100` (100ms interval)
pub fn build_subscription_channels(instruments: &[String], book_depth: u32) -> Vec<String> {
    let mut channels = Vec::with_capacity(instruments.len() * 2);

    for inst in instruments {
        channels.push(format!("orderbook.{}.10.{}", inst, book_depth));
        channels.push(format!("ticker_slim.{}.100", inst));
    }

    channels
}

/// Extract the instrument name from a channel string, if applicable.
///
/// Handles instrument names containing dots-free but with dashes (e.g.,
/// `BTC-20260305-69500-P`). Parsing strategy differs by channel type:
///
/// - **Orderbook** `orderbook.{inst}.{group}.{depth}`: strip prefix, then
///   `rsplitn(3, '.')` to remove the two trailing segments (group + depth).
/// - **Ticker slim** `ticker_slim.{inst}.{interval}`: strip prefix, then
///   `rsplit_once('.')` to remove the one trailing segment (interval).
pub fn extract_instrument(channel: &str) -> Option<String> {
    let kind = DeriveChannelKind::parse(channel);
    match kind {
        DeriveChannelKind::Orderbook => {
            // Format: orderbook.{instrument}.{group}.{depth}
            let rest = channel.strip_prefix("orderbook.")?;
            // rest = "BTC-20260305-69500-P.10.10"
            // rsplitn(3, '.') yields: ["10", "10", "BTC-20260305-69500-P"]
            let mut parts = rest.rsplitn(3, '.');
            let _depth = parts.next()?;
            let _group = parts.next()?;
            let instrument = parts.next()?;
            Some(instrument.to_string())
        }
        DeriveChannelKind::TickerSlim => {
            // Format: ticker_slim.{instrument}.{interval}
            let rest = channel.strip_prefix("ticker_slim.")?;
            // rest = "BTC-20260305-69500-P.100"
            let (instrument, _interval) = rest.rsplit_once('.')?;
            Some(instrument.to_string())
        }
        DeriveChannelKind::Unknown(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_orderbook_channel() {
        assert_eq!(
            DeriveChannelKind::parse("orderbook.BTC-20260305-69500-P.10.10"),
            DeriveChannelKind::Orderbook
        );
    }

    #[test]
    fn parse_ticker_slim_channel() {
        assert_eq!(
            DeriveChannelKind::parse("ticker_slim.BTC-20260305-69500-P.100"),
            DeriveChannelKind::TickerSlim
        );
    }

    #[test]
    fn parse_unknown_channel() {
        assert_eq!(
            DeriveChannelKind::parse("something.else"),
            DeriveChannelKind::Unknown("something.else".to_string())
        );
    }

    #[test]
    fn build_subscription_channels_produces_correct_names() {
        let instruments = vec![
            "BTC-20260305-69500-P".to_string(),
            "BTC-20260308-71000-C".to_string(),
        ];
        let channels = build_subscription_channels(&instruments, 10);

        assert_eq!(channels.len(), 4);
        assert_eq!(channels[0], "orderbook.BTC-20260305-69500-P.10.10");
        assert_eq!(channels[1], "ticker_slim.BTC-20260305-69500-P.100");
        assert_eq!(channels[2], "orderbook.BTC-20260308-71000-C.10.10");
        assert_eq!(channels[3], "ticker_slim.BTC-20260308-71000-C.100");
    }

    #[test]
    fn build_subscription_channels_empty_instruments() {
        let instruments: Vec<String> = vec![];
        let channels = build_subscription_channels(&instruments, 10);
        assert!(channels.is_empty());
    }

    #[test]
    fn extract_instrument_from_orderbook() {
        assert_eq!(
            extract_instrument("orderbook.BTC-20260305-69500-P.10.10"),
            Some("BTC-20260305-69500-P".to_string())
        );
    }

    #[test]
    fn extract_instrument_from_ticker_slim() {
        assert_eq!(
            extract_instrument("ticker_slim.BTC-20260305-69500-P.100"),
            Some("BTC-20260305-69500-P".to_string())
        );
    }

    #[test]
    fn extract_instrument_from_unknown_returns_none() {
        assert_eq!(extract_instrument("something.else"), None);
    }

    #[test]
    fn extract_instrument_handles_dashes_in_name() {
        // Instrument names contain dashes: BTC-20260305-69500-P
        // Ensure the parser doesn't split on dashes
        assert_eq!(
            extract_instrument("orderbook.BTC-20260305-69500-P.10.10"),
            Some("BTC-20260305-69500-P".to_string())
        );
        assert_eq!(
            extract_instrument("ticker_slim.BTC-20260305-69500-P.100"),
            Some("BTC-20260305-69500-P".to_string())
        );
    }

    #[test]
    fn build_subscription_channels_different_depth() {
        let instruments = vec!["BTC-20260305-69500-P".to_string()];
        let channels = build_subscription_channels(&instruments, 20);

        assert_eq!(channels[0], "orderbook.BTC-20260305-69500-P.10.20");
    }
}
