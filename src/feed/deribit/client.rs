//! Deribit WebSocket client.
//!
//! Connects to the Deribit WebSocket API, subscribes to market data channels,
//! and forwards raw text frames through an mpsc channel. No reconnection logic
//! in Phase 2 -- that is Phase 3.

use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::config::DeribitConfig;
use crate::feed::deribit::channels;
use crate::feed::traits::RawMessage;
use crate::types::DualTimestamp;

/// Buffer size for the raw message channel.
/// Per research: raw frames are small (~1-5KB); buffer absorbs parsing latency spikes.
const RAW_MESSAGE_BUFFER: usize = 1024;

/// Atomic counter for JSON-RPC request IDs.
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Deribit WebSocket client.
///
/// Connects to Deribit, subscribes to all 4 channel types for a list of
/// instruments, and forwards raw text frames through an mpsc channel.
pub struct DeribitClient {
    config: DeribitConfig,
    instruments: Vec<String>,
    cancel: CancellationToken,
}

impl DeribitClient {
    /// Create a new `DeribitClient`.
    ///
    /// - `config`: Deribit connection settings (ws_url, etc.)
    /// - `instruments`: List of instrument names to subscribe to
    /// - `cancel`: Cancellation token for graceful shutdown
    pub fn new(
        config: DeribitConfig,
        instruments: Vec<String>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            config,
            instruments,
            cancel,
        }
    }

    /// Connect to Deribit, subscribe to all channels, and start reading.
    ///
    /// Returns an `mpsc::Receiver<RawMessage>` that receives raw WebSocket
    /// text frames as they arrive. Spawns a background tokio task for the
    /// read loop.
    ///
    /// If the initial connection fails, returns an error immediately.
    /// The caller decides whether to retry (Phase 3) or fail.
    pub async fn start(&self) -> anyhow::Result<mpsc::Receiver<RawMessage>> {
        let ws_url = self.config.ws_url.clone();
        tracing::info!(url = %ws_url, "connecting to Deribit WebSocket");

        let (ws_stream, _response) =
            tokio_tungstenite::connect_async(&ws_url)
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, url = %ws_url, "failed to connect to Deribit");
                    anyhow::anyhow!("WebSocket connection failed: {}", e)
                })?;

        tracing::info!(url = %ws_url, "connected to Deribit WebSocket");

        let (mut write, mut read) = ws_stream.split();

        // Build subscription channel list
        let subscription_channels = channels::build_subscription_channels(&self.instruments);
        let channel_count = subscription_channels.len();

        // Send a single batch subscribe request (avoids per-channel rate limit issues)
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let subscribe_msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "public/subscribe",
            "params": {
                "channels": subscription_channels
            }
        });

        tracing::info!(
            channels = channel_count,
            request_id = request_id,
            "subscribing to Deribit channels"
        );

        write
            .send(Message::text(subscribe_msg.to_string()))
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "failed to send subscribe request");
                anyhow::anyhow!("subscribe request failed: {}", e)
            })?;

        // Create the raw message channel
        let (tx, rx) = mpsc::channel::<RawMessage>(RAW_MESSAGE_BUFFER);

        // Spawn the read loop as a background task
        let cancel = self.cancel.clone();
        tokio::spawn(async move {
            tracing::debug!("Deribit WS read loop started");

            loop {
                tokio::select! {
                    biased;

                    _ = cancel.cancelled() => {
                        tracing::info!("Deribit WS read loop cancelled");
                        // Attempt to send close frame
                        let _ = write.send(Message::Close(None)).await;
                        break;
                    }

                    msg = read.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                let raw = RawMessage {
                                    text: text.to_string(),
                                    received_at: DualTimestamp::now(),
                                };

                                if tx.send(raw).await.is_err() {
                                    tracing::warn!("raw message receiver dropped, stopping read loop");
                                    break;
                                }
                            }
                            Some(Ok(Message::Close(frame))) => {
                                let reason = frame
                                    .as_ref()
                                    .map(|f| f.reason.to_string())
                                    .unwrap_or_else(|| "no reason".to_string());
                                tracing::info!(reason = %reason, "Deribit WS connection closed by server");
                                break;
                            }
                            Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                                // tokio-tungstenite handles pong automatically.
                                // We just keep reading (pitfall 2 from research).
                            }
                            Some(Ok(Message::Binary(_))) => {
                                tracing::debug!("ignoring binary WS frame");
                            }
                            Some(Ok(Message::Frame(_))) => {
                                // Raw frame -- ignore.
                            }
                            Some(Err(e)) => {
                                tracing::error!(error = %e, "Deribit WS read error");
                                break;
                            }
                            None => {
                                tracing::info!("Deribit WS stream ended");
                                break;
                            }
                        }
                    }
                }
            }

            tracing::debug!("Deribit WS read loop exiting");
        });

        Ok(rx)
    }
}

/// Implement `RawDataSource` for `DeribitClient`.
impl crate::feed::traits::RawDataSource for DeribitClient {
    async fn start(&self) -> anyhow::Result<mpsc::Receiver<RawMessage>> {
        DeribitClient::start(self).await
    }
}
