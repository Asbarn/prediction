//! Deribit channel name construction and channel-type routing.
//!
//! Deribit subscription channels follow patterns like:
//! - `book.{instrument}.none.20.100ms`
//! - `ticker.{instrument}.raw`
//! - `trades.{instrument}.raw`
//! - `deribit_price_index.btc_usd`

/// The kind of a Deribit subscription channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelKind {
    /// Order book snapshots: `book.{instrument}.none.20.100ms`
    Book,
    /// Ticker data: `ticker.{instrument}.raw`
    Ticker,
    /// Trade data: `trades.{instrument}.raw`
    Trades,
    /// Price index: `deribit_price_index.{index_name}`
    PriceIndex,
    /// Unknown channel type.
    Unknown(String),
}

impl ChannelKind {
    /// Parse a channel name string into a `ChannelKind`.
    ///
    /// # Examples
    /// ```
    /// use prediction::feed::deribit::channels::ChannelKind;
    ///
    /// assert_eq!(
    ///     ChannelKind::parse("book.BTC-27JUN25-100000-C.none.20.100ms"),
    ///     ChannelKind::Book,
    /// );
    /// assert_eq!(
    ///     ChannelKind::parse("ticker.BTC-27JUN25-100000-C.raw"),
    ///     ChannelKind::Ticker,
    /// );
    /// ```
    pub fn parse(channel: &str) -> Self {
        if channel.starts_with("book.") {
            ChannelKind::Book
        } else if channel.starts_with("ticker.") {
            ChannelKind::Ticker
        } else if channel.starts_with("trades.") {
            ChannelKind::Trades
        } else if channel.starts_with("deribit_price_index.") {
            ChannelKind::PriceIndex
        } else {
            ChannelKind::Unknown(channel.to_string())
        }
    }
}

/// Extract the instrument name from a channel string, if applicable.
///
/// Returns `Some(instrument)` for book, ticker, and trades channels.
/// Returns `None` for price index and unknown channels.
///
/// # Examples
/// ```
/// use prediction::feed::deribit::channels::extract_instrument;
///
/// assert_eq!(
///     extract_instrument("ticker.BTC-27JUN25-100000-C.raw"),
///     Some("BTC-27JUN25-100000-C".to_string()),
/// );
/// assert_eq!(
///     extract_instrument("deribit_price_index.btc_usd"),
///     None,
/// );
/// ```
pub fn extract_instrument(channel: &str) -> Option<String> {
    let kind = ChannelKind::parse(channel);
    match kind {
        ChannelKind::Book => {
            // Format: book.{instrument}.none.20.100ms
            // Split by '.' and take everything between first and last 3 parts
            let parts: Vec<&str> = channel.splitn(2, '.').collect();
            if parts.len() < 2 {
                return None;
            }
            let rest = parts[1];
            // rest = "BTC-27JUN25-100000-C.none.20.100ms"
            // We need to strip ".none.20.100ms" from the end
            let suffix = ".none.";
            if let Some(pos) = rest.find(suffix) {
                Some(rest[..pos].to_string())
            } else {
                None
            }
        }
        ChannelKind::Ticker => {
            // Format: ticker.{instrument}.raw
            let rest = channel.strip_prefix("ticker.")?;
            let instrument = rest.strip_suffix(".raw")?;
            Some(instrument.to_string())
        }
        ChannelKind::Trades => {
            // Format: trades.{instrument}.raw
            let rest = channel.strip_prefix("trades.")?;
            let instrument = rest.strip_suffix(".raw")?;
            Some(instrument.to_string())
        }
        ChannelKind::PriceIndex | ChannelKind::Unknown(_) => None,
    }
}

/// Build the full set of subscription channel names for a list of instruments.
///
/// For each instrument, produces:
/// - `book.{instrument}.none.20.100ms`
/// - `ticker.{instrument}.raw`
/// - `trades.{instrument}.raw`
///
/// Additionally includes `deribit_price_index.btc_usd` (always subscribed).
pub fn build_subscription_channels(instruments: &[String]) -> Vec<String> {
    let mut channels = Vec::with_capacity(instruments.len() * 3 + 1);

    for inst in instruments {
        channels.push(format!("book.{}.none.20.100ms", inst));
        channels.push(format!("ticker.{}.raw", inst));
        channels.push(format!("trades.{}.raw", inst));
    }

    channels.push("deribit_price_index.btc_usd".to_string());

    channels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_book_channel() {
        assert_eq!(
            ChannelKind::parse("book.BTC-27JUN25-100000-C.none.20.100ms"),
            ChannelKind::Book
        );
    }

    #[test]
    fn parse_ticker_channel() {
        assert_eq!(
            ChannelKind::parse("ticker.BTC-27JUN25-100000-C.raw"),
            ChannelKind::Ticker
        );
    }

    #[test]
    fn parse_trades_channel() {
        assert_eq!(
            ChannelKind::parse("trades.BTC-27JUN25-100000-C.raw"),
            ChannelKind::Trades
        );
    }

    #[test]
    fn parse_price_index_channel() {
        assert_eq!(
            ChannelKind::parse("deribit_price_index.btc_usd"),
            ChannelKind::PriceIndex
        );
    }

    #[test]
    fn parse_unknown_channel() {
        assert_eq!(
            ChannelKind::parse("something.else"),
            ChannelKind::Unknown("something.else".to_string())
        );
    }

    #[test]
    fn extract_instrument_from_book() {
        assert_eq!(
            extract_instrument("book.BTC-27JUN25-100000-C.none.20.100ms"),
            Some("BTC-27JUN25-100000-C".to_string())
        );
    }

    #[test]
    fn extract_instrument_from_ticker() {
        assert_eq!(
            extract_instrument("ticker.BTC-27JUN25-100000-C.raw"),
            Some("BTC-27JUN25-100000-C".to_string())
        );
    }

    #[test]
    fn extract_instrument_from_trades() {
        assert_eq!(
            extract_instrument("trades.ETH-PERPETUAL.raw"),
            Some("ETH-PERPETUAL".to_string())
        );
    }

    #[test]
    fn extract_instrument_from_price_index_returns_none() {
        assert_eq!(extract_instrument("deribit_price_index.btc_usd"), None);
    }

    #[test]
    fn extract_instrument_from_unknown_returns_none() {
        assert_eq!(extract_instrument("something.else"), None);
    }

    #[test]
    fn build_subscription_channels_single_instrument() {
        let instruments = vec!["BTC-27JUN25-100000-C".to_string()];
        let channels = build_subscription_channels(&instruments);

        assert_eq!(channels.len(), 4);
        assert_eq!(channels[0], "book.BTC-27JUN25-100000-C.none.20.100ms");
        assert_eq!(channels[1], "ticker.BTC-27JUN25-100000-C.raw");
        assert_eq!(channels[2], "trades.BTC-27JUN25-100000-C.raw");
        assert_eq!(channels[3], "deribit_price_index.btc_usd");
    }

    #[test]
    fn build_subscription_channels_multiple_instruments() {
        let instruments = vec![
            "BTC-27JUN25-100000-C".to_string(),
            "BTC-27JUN25-80000-P".to_string(),
        ];
        let channels = build_subscription_channels(&instruments);

        // 3 channels per instrument + 1 price index = 7
        assert_eq!(channels.len(), 7);
        assert!(channels.contains(&"book.BTC-27JUN25-80000-P.none.20.100ms".to_string()));
        assert!(channels.contains(&"ticker.BTC-27JUN25-80000-P.raw".to_string()));
        assert!(channels.contains(&"trades.BTC-27JUN25-80000-P.raw".to_string()));
        // Price index should only appear once
        assert_eq!(
            channels
                .iter()
                .filter(|c| c.as_str() == "deribit_price_index.btc_usd")
                .count(),
            1
        );
    }

    #[test]
    fn build_subscription_channels_empty_instruments() {
        let instruments: Vec<String> = vec![];
        let channels = build_subscription_channels(&instruments);

        // Just the price index
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0], "deribit_price_index.btc_usd");
    }
}
