//! Polymarket CLOB WebSocket client.
//!
//! Connects to the Polymarket market channel, subscribes to order book updates
//! for configured token IDs, and forwards raw text frames through an mpsc
//! channel. Handles Polymarket's PING heartbeat protocol (send WebSocket PING
//! every 10 seconds to keep the connection alive).
//!
//! No authentication is needed for the market channel (public data).
//! Reconnection logic lives in the supervisor, not here.

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::config::PolymarketConfig;
use crate::feed::traits::RawMessage;
use crate::types::DualTimestamp;

/// Buffer size for the raw message channel.
/// Same as Deribit -- raw frames are small; buffer absorbs parsing latency spikes.
const RAW_MESSAGE_BUFFER: usize = 1024;

/// Polymarket CLOB WebSocket client.
///
/// Connects to the Polymarket market channel WebSocket, subscribes to order
/// book updates for the configured token IDs, and forwards raw text frames
/// through an mpsc channel.
pub struct PolymarketClient {
    config: PolymarketConfig,
    cancel: CancellationToken,
}

impl PolymarketClient {
    /// Create a new `PolymarketClient`.
    pub fn new(config: PolymarketConfig, cancel: CancellationToken) -> Self {
        Self { config, cancel }
    }

    /// Connect to Polymarket, subscribe to the market channel, and start reading.
    ///
    /// Returns an `mpsc::Receiver<RawMessage>` that receives raw WebSocket
    /// text frames as they arrive. Spawns background tasks for reading and
    /// sending periodic PING frames.
    ///
    /// The market channel requires no authentication. Subscription uses the
    /// `assets_ids` field with token IDs (NOT condition IDs -- see Pitfall 1).
    pub async fn start(&self) -> anyhow::Result<mpsc::Receiver<RawMessage>> {
        let ws_url = self.config.ws_url.clone();
        tracing::info!(url = %ws_url, "connecting to Polymarket WebSocket");

        let (ws_stream, _response) =
            tokio_tungstenite::connect_async(&ws_url)
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, url = %ws_url, "failed to connect to Polymarket");
                    anyhow::anyhow!("Polymarket WebSocket connection failed: {}", e)
                })?;

        tracing::info!(url = %ws_url, "connected to Polymarket WebSocket");

        let (mut write, mut read) = ws_stream.split();

        // Build subscription message with token IDs from config
        let token_ids: Vec<&str> = self
            .config
            .assets
            .iter()
            .map(|a| a.token_id.as_str())
            .collect();

        let subscribe_msg = serde_json::json!({
            "assets_ids": token_ids,
            "type": "market"
        });

        tracing::info!(
            assets = token_ids.len(),
            "subscribing to Polymarket market channel"
        );

        write
            .send(Message::text(subscribe_msg.to_string()))
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "failed to send Polymarket subscribe request");
                anyhow::anyhow!("Polymarket subscribe request failed: {}", e)
            })?;

        // Create the raw message channel
        let (tx, rx) = mpsc::channel::<RawMessage>(RAW_MESSAGE_BUFFER);

        let ping_interval = Duration::from_millis(self.config.ping_interval_ms);
        let cancel = self.cancel.clone();

        // Spawn the bidirectional WS loop as a background task.
        tokio::spawn(async move {
            tracing::debug!("Polymarket WS loop started");

            let mut ping_timer = tokio::time::interval(ping_interval);
            // The first tick fires immediately; skip it so we wait a full interval.
            ping_timer.tick().await;

            loop {
                tokio::select! {
                    biased;

                    _ = cancel.cancelled() => {
                        tracing::info!("Polymarket WS loop cancelled");
                        let _ = write.send(Message::Close(None)).await;
                        break;
                    }

                    // Send PING at configured interval to keep connection alive
                    _ = ping_timer.tick() => {
                        if let Err(e) = write.send(Message::Ping(vec![].into())).await {
                            tracing::error!(error = %e, "failed to send Polymarket PING");
                            break;
                        }
                        tracing::trace!("sent Polymarket PING");
                    }

                    msg = read.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                let text_str = text.to_string();

                                let raw = RawMessage {
                                    text: text_str,
                                    received_at: DualTimestamp::now(),
                                };

                                if tx.send(raw).await.is_err() {
                                    tracing::warn!("Polymarket raw message receiver dropped, stopping WS loop");
                                    break;
                                }
                            }
                            Some(Ok(Message::Close(frame))) => {
                                let reason = frame
                                    .as_ref()
                                    .map(|f| f.reason.to_string())
                                    .unwrap_or_else(|| "no reason".to_string());
                                tracing::info!(reason = %reason, "Polymarket WS connection closed by server");
                                break;
                            }
                            Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                                // tokio-tungstenite handles pong automatically for Ping.
                                // Pong is a response to our Ping -- connection is alive.
                                tracing::trace!("Polymarket WS ping/pong");
                            }
                            Some(Ok(Message::Binary(_))) => {
                                tracing::debug!("ignoring Polymarket binary WS frame");
                            }
                            Some(Ok(Message::Frame(_))) => {
                                // Raw frame -- ignore.
                            }
                            Some(Err(e)) => {
                                tracing::error!(error = %e, "Polymarket WS read error");
                                break;
                            }
                            None => {
                                tracing::info!("Polymarket WS stream ended");
                                break;
                            }
                        }
                    }
                }
            }

            tracing::debug!("Polymarket WS loop exiting");
        });

        Ok(rx)
    }
}

/// Implement `RawDataSource` for `PolymarketClient`.
impl crate::feed::traits::RawDataSource for PolymarketClient {
    async fn start(&self) -> anyhow::Result<mpsc::Receiver<RawMessage>> {
        PolymarketClient::start(self).await
    }
}
