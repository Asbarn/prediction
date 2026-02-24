//! Kalshi WebSocket client.
//!
//! Connects to the Kalshi WebSocket API with RSA-PSS authentication headers,
//! subscribes to orderbook channels for configured market tickers, and forwards
//! raw text frames through an mpsc channel.
//!
//! Auth headers are generated fresh for each connection attempt (since the
//! timestamp is part of the signing message). Reconnection logic lives in
//! the supervisor, not here.

use futures_util::{SinkExt, StreamExt};
use rsa::RsaPrivateKey;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::config::KalshiConfig;
use crate::feed::kalshi::auth::sign_kalshi_request;
use crate::feed::traits::RawMessage;
use crate::types::DualTimestamp;

/// Buffer size for the raw message channel.
const RAW_MESSAGE_BUFFER: usize = 1024;

/// Kalshi WebSocket client.
///
/// Connects to Kalshi with RSA-PSS auth headers, subscribes to orderbook
/// channels for configured market tickers, and forwards raw text frames
/// through an mpsc channel.
pub struct KalshiClient {
    config: KalshiConfig,
    api_key_id: String,
    private_key: RsaPrivateKey,
    cancel: CancellationToken,
}

impl KalshiClient {
    /// Create a new `KalshiClient`.
    pub fn new(
        config: KalshiConfig,
        api_key_id: String,
        private_key: RsaPrivateKey,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            config,
            api_key_id,
            private_key,
            cancel,
        }
    }

    /// Connect to Kalshi, subscribe to orderbook channels, and start reading.
    ///
    /// Returns an `mpsc::Receiver<RawMessage>` that receives raw WebSocket
    /// text frames. Spawns a background tokio task that owns the connection.
    ///
    /// Authentication: generates a fresh RSA-PSS signature with current
    /// timestamp for the WebSocket handshake headers.
    pub async fn start(&self) -> anyhow::Result<mpsc::Receiver<RawMessage>> {
        let ws_url = self.config.ws_url.clone();
        tracing::info!(url = %ws_url, "connecting to Kalshi WebSocket");

        // Generate fresh auth signature (timestamp is part of signing message)
        let timestamp_ms = chrono::Utc::now().timestamp_millis();

        // Extract the path from the ws_url for signing
        let path = extract_ws_path(&ws_url);

        let signature = sign_kalshi_request(&self.private_key, timestamp_ms, "GET", &path)
            .map_err(|e| {
                tracing::error!(error = %e, "failed to sign Kalshi request");
                anyhow::anyhow!("Kalshi auth signing failed: {}", e)
            })?;

        // Build HTTP request with auth headers
        let request = http::Request::builder()
            .uri(&ws_url)
            .header("KALSHI-ACCESS-KEY", &self.api_key_id)
            .header("KALSHI-ACCESS-SIGNATURE", &signature)
            .header("KALSHI-ACCESS-TIMESTAMP", timestamp_ms.to_string())
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .header("Sec-WebSocket-Version", "13")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Host", extract_host(&ws_url))
            .body(())
            .map_err(|e| {
                tracing::error!(error = %e, "failed to build Kalshi WS request");
                anyhow::anyhow!("failed to build WebSocket request: {}", e)
            })?;

        let (ws_stream, _response) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, url = %ws_url, "failed to connect to Kalshi");
                anyhow::anyhow!("Kalshi WebSocket connection failed: {}", e)
            })?;

        tracing::info!(url = %ws_url, "connected to Kalshi WebSocket");

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to orderbook channels for each market ticker
        for (i, ticker) in self.config.market_tickers.iter().enumerate() {
            let subscribe_msg = serde_json::json!({
                "id": (i + 1) as i64,
                "cmd": "subscribe",
                "params": {
                    "channels": ["orderbook_delta"],
                    "market_ticker": ticker
                }
            });

            tracing::info!(
                ticker = %ticker,
                "subscribing to Kalshi orderbook channel"
            );

            write
                .send(Message::text(subscribe_msg.to_string()))
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, ticker = %ticker, "failed to send subscribe");
                    anyhow::anyhow!("Kalshi subscribe failed: {}", e)
                })?;
        }

        // Create the raw message channel
        let (tx, rx) = mpsc::channel::<RawMessage>(RAW_MESSAGE_BUFFER);

        // Spawn the WS reader loop as a background task
        let cancel = self.cancel.clone();
        let heartbeat_timeout_ms = self.config.heartbeat_timeout_ms;
        tokio::spawn(async move {
            tracing::debug!("Kalshi WS loop started");

            let timeout_duration = Duration::from_millis(heartbeat_timeout_ms);
            let mut last_message_at = Instant::now();

            loop {
                let timeout_deadline = last_message_at + timeout_duration;

                tokio::select! {
                    biased;

                    _ = cancel.cancelled() => {
                        tracing::info!("Kalshi WS loop cancelled");
                        let _ = write.send(Message::Close(None)).await;
                        break;
                    }

                    // Dead-connection timeout: no messages/pings received within threshold.
                    // Kalshi sends Ping every ~10s; timeout at 3x (30s default) detects dead connections.
                    // Supervisor will reconnect after this break.
                    _ = tokio::time::sleep_until(timeout_deadline) => {
                        let elapsed = last_message_at.elapsed();
                        tracing::warn!(
                            elapsed_ms = elapsed.as_millis() as u64,
                            timeout_ms = timeout_duration.as_millis() as u64,
                            "Kalshi heartbeat timeout -- no messages/pings received, connection assumed dead"
                        );
                        metrics::counter!("feed_heartbeat_timeouts", "venue" => "kalshi").increment(1);
                        break;
                    }

                    msg = read.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                last_message_at = Instant::now();
                                let text_str = text.to_string();

                                let raw = RawMessage {
                                    text: text_str,
                                    received_at: DualTimestamp::now(),
                                };

                                if tx.send(raw).await.is_err() {
                                    tracing::warn!("Kalshi raw message receiver dropped, stopping WS loop");
                                    break;
                                }
                            }
                            Some(Ok(Message::Close(frame))) => {
                                let reason = frame
                                    .as_ref()
                                    .map(|f| f.reason.to_string())
                                    .unwrap_or_else(|| "no reason".to_string());
                                tracing::info!(reason = %reason, "Kalshi WS connection closed by server");
                                break;
                            }
                            Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                                // tokio-tungstenite handles pong automatically.
                                // Update liveness tracker -- these ARE the Kalshi heartbeat.
                                last_message_at = Instant::now();
                            }
                            Some(Ok(Message::Binary(_))) => {
                                tracing::debug!("ignoring binary Kalshi WS frame");
                                last_message_at = Instant::now();
                            }
                            Some(Ok(Message::Frame(_))) => {
                                // Raw frame -- ignore but update liveness.
                                last_message_at = Instant::now();
                            }
                            Some(Err(e)) => {
                                tracing::error!(error = %e, "Kalshi WS read error");
                                break;
                            }
                            None => {
                                tracing::info!("Kalshi WS stream ended");
                                break;
                            }
                        }
                    }
                }
            }

            tracing::debug!("Kalshi WS loop exiting");
        });

        Ok(rx)
    }
}

/// Extract the path component from a WebSocket URL for signing.
///
/// E.g., "wss://trading-api.kalshi.com/trade-api/ws/v2" -> "/trade-api/ws/v2"
fn extract_ws_path(url: &str) -> String {
    // Find the path after the host
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        if let Some(path_start) = after_scheme.find('/') {
            return after_scheme[path_start..].to_string();
        }
    }
    "/".to_string()
}

/// Extract the host from a WebSocket URL for the Host header.
///
/// E.g., "wss://trading-api.kalshi.com/trade-api/ws/v2" -> "trading-api.kalshi.com"
fn extract_host(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        if let Some(path_start) = after_scheme.find('/') {
            return after_scheme[..path_start].to_string();
        }
        return after_scheme.to_string();
    }
    url.to_string()
}

/// Implement `RawDataSource` for `KalshiClient`.
impl crate::feed::traits::RawDataSource for KalshiClient {
    async fn start(&self) -> anyhow::Result<mpsc::Receiver<RawMessage>> {
        KalshiClient::start(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_ws_path_from_url() {
        assert_eq!(
            extract_ws_path("wss://trading-api.kalshi.com/trade-api/ws/v2"),
            "/trade-api/ws/v2"
        );
        assert_eq!(
            extract_ws_path("wss://api.elections.kalshi.com/trade-api/ws/v2"),
            "/trade-api/ws/v2"
        );
        assert_eq!(extract_ws_path("wss://example.com"), "/");
    }

    #[test]
    fn extract_host_from_url() {
        assert_eq!(
            extract_host("wss://trading-api.kalshi.com/trade-api/ws/v2"),
            "trading-api.kalshi.com"
        );
        assert_eq!(
            extract_host("wss://api.elections.kalshi.com/trade-api/ws/v2"),
            "api.elections.kalshi.com"
        );
    }
}
