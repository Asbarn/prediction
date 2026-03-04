//! Derive.xyz WebSocket client.
//!
//! Connects to the Derive WebSocket API, subscribes to market data channels
//! (orderbook + ticker_slim), and forwards raw text frames through an mpsc
//! channel. Unlike Deribit, Derive uses WS-level PING/PONG for keepalive --
//! no application-level heartbeat protocol needed. Dead connections are
//! detected via a simple 60-second message timeout.

use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::config::DeriveConfig;
use crate::feed::derive::channels;
use crate::feed::reliability::VenueRateLimiter;
use crate::feed::traits::RawMessage;
use crate::types::DualTimestamp;

/// Buffer size for the raw message channel.
/// Per research: raw frames are small (~200-300 bytes); buffer absorbs parsing latency spikes.
const RAW_MESSAGE_BUFFER: usize = 1024;

/// Dead connection timeout. Derive sends orderbook snapshots every ~100ms,
/// so 60 seconds with no messages means the connection is dead.
const DEAD_CONNECTION_TIMEOUT: Duration = Duration::from_secs(60);

/// Atomic counter for JSON-RPC request IDs.
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Derive WebSocket client.
///
/// Connects to Derive, subscribes to orderbook and ticker_slim channels for
/// a list of instruments, and forwards raw text frames through an mpsc channel.
/// No heartbeat protocol -- only a 60-second dead connection timeout.
pub struct DeriveClient {
    config: DeriveConfig,
    instruments: Vec<String>,
    cancel: CancellationToken,
    rate_limiter: Option<VenueRateLimiter>,
}

impl DeriveClient {
    /// Create a new `DeriveClient`.
    ///
    /// - `config`: Derive connection settings (ws_url, book_depth_levels, etc.)
    /// - `instruments`: List of instrument names to subscribe to
    /// - `cancel`: Cancellation token for graceful shutdown
    /// - `rate_limiter`: Optional rate limiter for outbound messages
    pub fn new(
        config: DeriveConfig,
        instruments: Vec<String>,
        cancel: CancellationToken,
        rate_limiter: Option<VenueRateLimiter>,
    ) -> Self {
        Self {
            config,
            instruments,
            cancel,
            rate_limiter,
        }
    }

    /// Connect to Derive, subscribe to all channels, and start reading.
    ///
    /// Returns an `mpsc::Receiver<RawMessage>` that receives raw WebSocket
    /// text frames as they arrive. Spawns a background tokio task that owns
    /// both the read and write halves of the WebSocket connection.
    ///
    /// The spawned task:
    /// 1. Sends a `subscribe` request for orderbook + ticker_slim channels
    /// 2. Reads incoming messages in a select loop
    /// 3. Detects dead connections when no messages arrive within 60 seconds
    /// 4. Forwards text frames to the raw message channel
    ///
    /// If the initial connection or subscribe fails, returns an error
    /// immediately. The caller decides whether to retry.
    pub async fn start(&self) -> anyhow::Result<mpsc::Receiver<RawMessage>> {
        let ws_url = self.config.ws_url.clone();
        tracing::info!(venue = "derive", url = %ws_url, "connecting to Derive WebSocket");

        let (ws_stream, _response) =
            tokio_tungstenite::connect_async(&ws_url)
                .await
                .map_err(|e| {
                    tracing::error!(venue = "derive", error = %e, url = %ws_url, "failed to connect to Derive");
                    anyhow::anyhow!("WebSocket connection failed: {}", e)
                })?;

        tracing::info!(venue = "derive", url = %ws_url, "connected to Derive WebSocket");

        let (mut write, mut read) = ws_stream.split();

        // Build subscription channel list (orderbook + ticker_slim per instrument)
        let subscription_channels =
            channels::build_subscription_channels(&self.instruments, self.config.book_depth_levels);
        let channel_count = subscription_channels.len();

        // Send a single batch subscribe request.
        // NOTE: method is "subscribe" (NOT "public/subscribe" like Deribit).
        let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let subscribe_msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "subscribe",
            "params": {
                "channels": subscription_channels
            }
        });

        tracing::info!(
            venue = "derive",
            channels = channel_count,
            request_id = request_id,
            "subscribing to Derive channels"
        );

        // Rate-limit the subscribe request (if rate limiter attached)
        if let Some(ref rl) = self.rate_limiter {
            rl.wait().await;
        }
        write
            .send(Message::text(subscribe_msg.to_string()))
            .await
            .map_err(|e| {
                tracing::error!(venue = "derive", error = %e, "failed to send subscribe request");
                anyhow::anyhow!("subscribe request failed: {}", e)
            })?;

        // Create the raw message channel
        let (tx, rx) = mpsc::channel::<RawMessage>(RAW_MESSAGE_BUFFER);

        // Spawn the read loop as a background task.
        // No heartbeat protocol -- just read frames and detect dead connections.
        let cancel = self.cancel.clone();
        tokio::spawn(async move {
            tracing::debug!(venue = "derive", "Derive WS read loop started");

            // Track the last time we received any message (for timeout detection).
            let mut last_message_at = Instant::now();

            loop {
                let timeout_deadline = last_message_at + DEAD_CONNECTION_TIMEOUT;

                tokio::select! {
                    biased;

                    _ = cancel.cancelled() => {
                        tracing::info!(venue = "derive", "Derive WS loop cancelled");
                        let _ = write.send(Message::Close(None)).await;
                        break;
                    }

                    // Dead connection timeout: no messages within 60 seconds.
                    _ = tokio::time::sleep_until(timeout_deadline) => {
                        let elapsed = last_message_at.elapsed();
                        tracing::warn!(
                            venue = "derive",
                            elapsed_ms = elapsed.as_millis() as u64,
                            timeout_ms = DEAD_CONNECTION_TIMEOUT.as_millis() as u64,
                            "dead connection timeout -- no messages received, connection assumed dead"
                        );
                        break;
                    }

                    msg = read.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                last_message_at = Instant::now();

                                let raw = RawMessage {
                                    text: text.to_string(),
                                    received_at: DualTimestamp::now(),
                                };

                                if tx.send(raw).await.is_err() {
                                    tracing::warn!(venue = "derive", "raw message receiver dropped, stopping WS loop");
                                    break;
                                }
                            }
                            Some(Ok(Message::Close(frame))) => {
                                let reason = frame
                                    .as_ref()
                                    .map(|f| f.reason.to_string())
                                    .unwrap_or_else(|| "no reason".to_string());
                                tracing::info!(venue = "derive", reason = %reason, "Derive WS connection closed by server");
                                break;
                            }
                            Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                                // tokio-tungstenite handles PONG automatically.
                                // Update liveness tracker (server is alive).
                                last_message_at = Instant::now();
                            }
                            Some(Ok(Message::Binary(_))) => {
                                tracing::debug!(venue = "derive", "ignoring binary WS frame");
                                last_message_at = Instant::now();
                            }
                            Some(Ok(Message::Frame(_))) => {
                                last_message_at = Instant::now();
                            }
                            Some(Err(e)) => {
                                tracing::error!(venue = "derive", error = %e, "Derive WS read error");
                                break;
                            }
                            None => {
                                tracing::info!(venue = "derive", "Derive WS stream ended");
                                break;
                            }
                        }
                    }
                }
            }

            tracing::debug!(venue = "derive", "Derive WS loop exiting");
        });

        Ok(rx)
    }
}

/// Implement `RawDataSource` for `DeriveClient`.
impl crate::feed::traits::RawDataSource for DeriveClient {
    async fn start(&self) -> anyhow::Result<mpsc::Receiver<RawMessage>> {
        DeriveClient::start(self).await
    }
}
