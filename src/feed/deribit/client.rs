//! Deribit WebSocket client.
//!
//! Connects to the Deribit WebSocket API, subscribes to market data channels,
//! and forwards raw text frames through an mpsc channel. Handles the Deribit
//! heartbeat protocol (set_heartbeat + test_request response) to keep the
//! connection alive. Detects dead connections via heartbeat timeout.
//!
//! Reconnection logic lives in the supervisor (Plan 03-02), not here.

use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::config::DeribitConfig;
use crate::feed::deribit::channels;
use crate::feed::reliability::VenueRateLimiter;
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
/// instruments, handles the heartbeat protocol, and forwards raw text
/// frames through an mpsc channel.
pub struct DeribitClient {
    config: DeribitConfig,
    instruments: Vec<String>,
    cancel: CancellationToken,
    rate_limiter: Option<VenueRateLimiter>,
}

impl DeribitClient {
    /// Create a new `DeribitClient`.
    ///
    /// - `config`: Deribit connection settings (ws_url, heartbeat_interval_ms, etc.)
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
            rate_limiter: None,
        }
    }

    /// Attach a rate limiter for outbound message throttling.
    ///
    /// When set, `wait()` is called before sending subscribe and
    /// set_heartbeat requests. Heartbeat test_request responses
    /// (`public/test`) are exempt per research pitfall 6.
    pub fn with_rate_limiter(mut self, limiter: VenueRateLimiter) -> Self {
        self.rate_limiter = Some(limiter);
        self
    }

    /// Connect to Deribit, subscribe to all channels, and start reading.
    ///
    /// Returns an `mpsc::Receiver<RawMessage>` that receives raw WebSocket
    /// text frames as they arrive. Spawns a background tokio task that owns
    /// both the read and write halves of the WebSocket connection.
    ///
    /// The spawned task:
    /// 1. Sends `public/set_heartbeat` to enable heartbeat monitoring
    /// 2. Reads incoming messages in a select loop
    /// 3. Responds to heartbeat `test_request` messages with `public/test`
    /// 4. Detects dead connections when no messages arrive within 2x the
    ///    heartbeat interval (exits the loop for supervisor to reconnect)
    /// 5. Forwards non-heartbeat text frames to the raw message channel
    ///
    /// If the initial connection or subscribe fails, returns an error
    /// immediately. The caller decides whether to retry.
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
        let subscription_channels = channels::build_subscription_channels(&self.instruments, self.config.book_depth_levels);
        let channel_count = subscription_channels.len();

        // Batch subscribe requests to stay within Deribit's 32KB message limit.
        // Each channel name is ~30-40 bytes; 400 channels per batch stays well under.
        const BATCH_SIZE: usize = 400;
        let batches: Vec<&[String]> = subscription_channels.chunks(BATCH_SIZE).collect();

        tracing::info!(
            channels = channel_count,
            batches = batches.len(),
            "subscribing to Deribit channels"
        );

        for (i, batch) in batches.iter().enumerate() {
            let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
            let subscribe_msg = serde_json::json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "public/subscribe",
                "params": {
                    "channels": batch
                }
            });

            if let Some(ref rl) = self.rate_limiter {
                rl.wait().await;
            }
            write
                .send(Message::text(subscribe_msg.to_string()))
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, batch = i, "failed to send subscribe request");
                    anyhow::anyhow!("subscribe request failed: {}", e)
                })?;

            tracing::debug!(
                batch = i + 1,
                batch_channels = batch.len(),
                request_id = request_id,
                "sent subscription batch"
            );
        }

        // Create the raw message channel
        let (tx, rx) = mpsc::channel::<RawMessage>(RAW_MESSAGE_BUFFER);

        // Compute heartbeat timeout: 2x the heartbeat interval.
        // Deribit minimum heartbeat interval is 10 seconds.
        let heartbeat_interval_ms = self.config.heartbeat_interval_ms.max(10_000);
        let heartbeat_interval_secs = (heartbeat_interval_ms / 1000) as u32;
        let timeout_duration = Duration::from_millis(heartbeat_interval_ms * 2);

        // Spawn the bidirectional WS loop as a background task.
        // The task owns both read and write halves (single owner for write).
        let cancel = self.cancel.clone();
        let rate_limiter = self.rate_limiter.clone();
        tokio::spawn(async move {
            tracing::debug!("Deribit WS loop started (bidirectional)");

            // Send public/set_heartbeat to enable server-side heartbeat monitoring.
            // The server will periodically send heartbeat notifications; if we fail
            // to respond to test_request messages, the server closes the connection.
            let hb_request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
            let set_heartbeat_msg = serde_json::json!({
                "jsonrpc": "2.0",
                "id": hb_request_id,
                "method": "public/set_heartbeat",
                "params": {
                    "interval": heartbeat_interval_secs
                }
            });

            tracing::info!(
                interval_s = heartbeat_interval_secs,
                request_id = hb_request_id,
                "sending public/set_heartbeat"
            );

            // Rate-limit the set_heartbeat request (if rate limiter attached)
            if let Some(ref rl) = rate_limiter {
                rl.wait().await;
            }
            if let Err(e) = write.send(Message::text(set_heartbeat_msg.to_string())).await {
                tracing::error!(error = %e, "failed to send set_heartbeat request");
                return;
            }

            // Track the last time we received any message (for timeout detection).
            let mut last_message_at = Instant::now();

            loop {
                // Compute the deadline for the heartbeat timeout.
                let timeout_deadline = last_message_at + timeout_duration;

                tokio::select! {
                    biased;

                    _ = cancel.cancelled() => {
                        tracing::info!("Deribit WS loop cancelled");
                        // Attempt to send close frame
                        let _ = write.send(Message::Close(None)).await;
                        break;
                    }

                    // Heartbeat timeout: no messages received within 2x heartbeat interval.
                    // The connection is dead -- exit for the supervisor to reconnect.
                    _ = tokio::time::sleep_until(timeout_deadline) => {
                        let elapsed = last_message_at.elapsed();
                        tracing::warn!(
                            elapsed_ms = elapsed.as_millis() as u64,
                            timeout_ms = timeout_duration.as_millis() as u64,
                            "heartbeat timeout -- no messages received, connection assumed dead"
                        );
                        break;
                    }

                    msg = read.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                // Update liveness tracker on every received message.
                                last_message_at = Instant::now();

                                let text_str = text.to_string();

                                // Check if this is a heartbeat message (fast string check).
                                // Heartbeat messages are connection-level protocol messages
                                // and must NOT be forwarded to the raw message channel.
                                if text_str.contains("\"method\":\"heartbeat\"")
                                    || text_str.contains("\"method\": \"heartbeat\"")
                                {
                                    // Parse to check if it's a test_request that needs response.
                                    if text_str.contains("\"test_request\"") {
                                        let test_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
                                        let test_response = serde_json::json!({
                                            "jsonrpc": "2.0",
                                            "id": test_id,
                                            "method": "public/test",
                                            "params": {}
                                        });

                                        tracing::debug!(
                                            request_id = test_id,
                                            "responding to heartbeat test_request with public/test"
                                        );

                                        // Send immediately -- exempt from rate limiting
                                        // (per research pitfall 6: heartbeat responses must
                                        // be prompt, never rate-limited).
                                        if let Err(e) = write.send(Message::text(test_response.to_string())).await {
                                            tracing::error!(error = %e, "failed to send heartbeat test response");
                                            break;
                                        }
                                    } else {
                                        tracing::debug!("received heartbeat keepalive (no response needed)");
                                    }
                                    // Do NOT forward heartbeat to raw_tx channel.
                                    continue;
                                }

                                // Non-heartbeat text message -- forward to raw channel.
                                let raw = RawMessage {
                                    text: text_str,
                                    received_at: DualTimestamp::now(),
                                };

                                if tx.send(raw).await.is_err() {
                                    tracing::warn!("raw message receiver dropped, stopping WS loop");
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
                                // Update liveness tracker.
                                last_message_at = Instant::now();
                            }
                            Some(Ok(Message::Binary(_))) => {
                                tracing::debug!("ignoring binary WS frame");
                                last_message_at = Instant::now();
                            }
                            Some(Ok(Message::Frame(_))) => {
                                // Raw frame -- ignore but update liveness.
                                last_message_at = Instant::now();
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

            tracing::debug!("Deribit WS loop exiting");
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
