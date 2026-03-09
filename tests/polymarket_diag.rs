//! Polymarket WebSocket diagnostic integration test.
//!
//! Connects to Polymarket's CLOB WebSocket and REST APIs to diagnose the
//! failure mode from the current host. Reports one of five outcomes:
//!
//! - **WORKING**: WS connection succeeded and data is flowing
//! - **CONNECTION_FAILED**: TCP/TLS connection refused or reset (geo-block?)
//! - **SILENT_FREEZE**: Connected and subscribed but no data arrives (GitHub #292)
//! - **READ_ERROR**: Connection established but read failed
//! - **CLOSED_BY_SERVER**: Server closed the connection after subscribe
//!
//! Run from EC2 (or any host) with:
//!
//! ```bash
//! cargo test --test polymarket_diag -- --ignored --nocapture
//! ```

#[cfg(test)]
mod polymarket_diag {
    use futures_util::{SinkExt, StreamExt};
    use tokio::time::Duration;
    use tokio_tungstenite::tungstenite::Message;

    /// Polymarket CLOB WebSocket endpoint.
    const WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";

    /// Polymarket CLOB REST base URL.
    const REST_URL: &str = "https://clob.polymarket.com";

    /// Gamma API URL for fetching active markets.
    const GAMMA_API_URL: &str = "https://gamma-api.polymarket.com";

    /// Fallback well-known token ID (2024 US Presidential Election market).
    const FALLBACK_TOKEN_ID: &str =
        "71321045679252212594626385532706912750332728571942532289631379312455583992563";

    /// How long to wait for the first WS data message.
    const DATA_TIMEOUT: Duration = Duration::from_secs(30);

    /// Fetch an active token_id from the Gamma API, falling back to a hardcoded one.
    async fn fetch_active_token_id(client: &reqwest::Client) -> String {
        println!("[DIAG] Fetching active market from Gamma API...");

        let url = format!("{GAMMA_API_URL}/markets?closed=false&limit=1");
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(body) = resp.text().await {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
                        // Response is an array of markets.
                        let markets = if parsed.is_array() {
                            parsed.as_array().cloned()
                        } else {
                            parsed.get("data").and_then(|d| d.as_array().cloned())
                        };

                        if let Some(markets) = markets {
                            if let Some(market) = markets.first() {
                                // Extract clob_token_ids[0].
                                if let Some(token_ids) = market.get("clobTokenIds").and_then(|t| t.as_array()) {
                                    if let Some(token_id) = token_ids.first().and_then(|t| t.as_str()) {
                                        println!("[DIAG] Found active token_id: {}", &token_id[..token_id.len().min(40)]);
                                        return token_id.to_string();
                                    }
                                }
                                // Try alternate field name.
                                if let Some(token_ids) = market.get("clob_token_ids").and_then(|t| t.as_array()) {
                                    if let Some(token_id) = token_ids.first().and_then(|t| t.as_str()) {
                                        println!("[DIAG] Found active token_id (snake_case): {}", &token_id[..token_id.len().min(40)]);
                                        return token_id.to_string();
                                    }
                                }
                                println!("[DIAG] WARNING: Market found but no clob_token_ids. Market keys: {:?}",
                                    market.as_object().map(|o| o.keys().collect::<Vec<_>>()));
                            }
                        }
                    }
                }
                println!("[DIAG] WARNING: Could not parse active market from Gamma API, using fallback token_id");
            }
            Ok(resp) => {
                println!("[DIAG] WARNING: Gamma API returned status {}. Using fallback token_id", resp.status());
            }
            Err(e) => {
                println!("[DIAG] WARNING: Gamma API request failed: {e}. Using fallback token_id");
            }
        }

        println!("[DIAG] Using fallback token_id: {}...", &FALLBACK_TOKEN_ID[..40]);
        FALLBACK_TOKEN_ID.to_string()
    }

    #[tokio::test]
    #[ignore] // Requires network access -- run manually from EC2 or any host
    async fn diagnose_polymarket_ws_from_this_host() {
        println!("\n========================================");
        println!("  POLYMARKET WS DIAGNOSTIC");
        println!("========================================\n");

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");

        // Step 1: REST baseline check.
        println!("[DIAG] === STEP 1: REST Baseline Check ===");
        let token_id = fetch_active_token_id(&client).await;

        let midpoint_url = format!("{REST_URL}/midpoint?token_id={token_id}");
        println!("[DIAG] GET {midpoint_url}");

        match client.get(&midpoint_url).send().await {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_else(|_| "<read error>".to_string());
                println!("[DIAG] REST /midpoint status: {status}");
                println!("[DIAG] REST /midpoint body: {}", &body[..body.len().min(200)]);

                if status.is_success() {
                    println!("[DIAG] REST API is reachable and responding");
                } else {
                    println!("[DIAG] WARNING: REST API returned non-success status");
                }
            }
            Err(e) => {
                println!("[DIAG] REST /midpoint FAILED: {e}");
                println!("[DIAG] WARNING: Cannot reach Polymarket REST API from this host");
            }
        }

        // Step 2: WS connection test.
        println!("\n[DIAG] === STEP 2: WebSocket Connection Test ===");
        println!("[DIAG] Connecting to {WS_URL}...");

        let ws_result = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async(WS_URL),
        )
        .await;

        let ws = match ws_result {
            Ok(Ok((ws, response))) => {
                println!("[DIAG] WS connected (HTTP status: {})", response.status());
                ws
            }
            Ok(Err(e)) => {
                println!("DIAGNOSIS: Connection failed: {e}");
                println!("VERDICT: CONNECTION_FAILED");
                return;
            }
            Err(_) => {
                println!("DIAGNOSIS: Connection timed out after 10s");
                println!("VERDICT: CONNECTION_FAILED");
                return;
            }
        };

        let (mut write, mut read) = ws.split();

        // Step 3: Subscribe to market data.
        println!("\n[DIAG] === STEP 3: Subscribe to Market Data ===");
        let subscribe_msg = serde_json::json!({
            "type": "market",
            "assets_ids": [&token_id]
        });
        let subscribe_str = subscribe_msg.to_string();
        println!("[DIAG] Sending: {subscribe_str}");

        if let Err(e) = write.send(Message::text(subscribe_str)).await {
            println!("DIAGNOSIS: Failed to send subscribe message: {e}");
            println!("VERDICT: CONNECTION_FAILED");
            return;
        }
        println!("[DIAG] Subscribe message sent");

        // Step 4: Data reception test.
        println!("\n[DIAG] === STEP 4: Data Reception Test ===");
        println!("[DIAG] Waiting up to {}s for first data message...", DATA_TIMEOUT.as_secs());

        match tokio::time::timeout(DATA_TIMEOUT, read.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let text_str = text.to_string();
                let display = if text_str.len() > 200 {
                    format!("{}...[truncated, {} bytes]", &text_str[..200], text_str.len())
                } else {
                    text_str
                };
                println!("DIAGNOSIS: WS working! Received: {display}");
                println!("VERDICT: WORKING");
            }
            Ok(Some(Ok(Message::Binary(data)))) => {
                println!("DIAGNOSIS: WS working! Received binary message ({} bytes)", data.len());
                println!("VERDICT: WORKING");
            }
            Ok(Some(Ok(Message::Ping(_)))) => {
                println!("[DIAG] Received Ping frame (not data). Waiting for data...");
                // Try once more for actual data.
                match tokio::time::timeout(DATA_TIMEOUT, read.next()).await {
                    Ok(Some(Ok(Message::Text(text)))) => {
                        let text_str = text.to_string();
                        let display = if text_str.len() > 200 {
                            format!("{}...[truncated, {} bytes]", &text_str[..200], text_str.len())
                        } else {
                            text_str
                        };
                        println!("DIAGNOSIS: WS working! Received: {display}");
                        println!("VERDICT: WORKING");
                    }
                    Ok(None) | Err(_) => {
                        println!("DIAGNOSIS: Silent freeze detected -- connected and subscribed but no data in {}s (GitHub #292)", DATA_TIMEOUT.as_secs());
                        println!("VERDICT: SILENT_FREEZE");
                    }
                    Ok(Some(Ok(_))) => {
                        println!("DIAGNOSIS: Received non-text frames only, no market data");
                        println!("VERDICT: SILENT_FREEZE");
                    }
                    Ok(Some(Err(e))) => {
                        println!("DIAGNOSIS: Read error after connect: {e}");
                        println!("VERDICT: READ_ERROR");
                    }
                }
            }
            Ok(Some(Ok(Message::Close(frame)))) => {
                let reason = frame
                    .as_ref()
                    .map(|f| format!("code={}, reason={}", f.code, f.reason))
                    .unwrap_or_else(|| "no close frame".to_string());
                println!("DIAGNOSIS: Connection closed by server immediately after subscribe ({reason})");
                println!("VERDICT: CLOSED_BY_SERVER");
            }
            Ok(Some(Ok(_))) => {
                println!("DIAGNOSIS: Received unexpected frame type (not text/binary/close)");
                println!("VERDICT: SILENT_FREEZE");
            }
            Ok(Some(Err(e))) => {
                println!("DIAGNOSIS: Read error after connect: {e}");
                println!("VERDICT: READ_ERROR");
            }
            Ok(None) => {
                println!("DIAGNOSIS: Connection closed by server immediately after subscribe");
                println!("VERDICT: CLOSED_BY_SERVER");
            }
            Err(_) => {
                println!("DIAGNOSIS: Silent freeze detected -- connected and subscribed but no data in {}s (GitHub #292)", DATA_TIMEOUT.as_secs());
                println!("VERDICT: SILENT_FREEZE");
            }
        }

        println!("\n========================================");
        println!("  DIAGNOSTIC COMPLETE");
        println!("========================================");
    }
}
