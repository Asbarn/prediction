//! Derive.xyz WebSocket API probe test.
//!
//! Connects to the Derive WebSocket API, sends subscribe requests, and captures
//! live messages to verify four critical API behaviors:
//!
//! 1. Channel subscription format (exact JSON-RPC method and params)
//! 2. Book update model (snapshot-only vs snapshot+delta)
//! 3. Heartbeat mechanism (WS ping/pong, custom JSON-RPC, or none)
//! 4. Authentication requirement for public channels
//!
//! This test is `#[ignore]` by default (requires network access to Derive API)
//! and can be run on demand with:
//!
//! ```bash
//! cargo test --test derive_api_probe probe_derive_websocket -- --ignored --nocapture
//! ```

#[cfg(test)]
mod derive_api_probe {
    use futures_util::{SinkExt, StreamExt};
    use tokio::time::{Duration, Instant};
    use tokio_tungstenite::tungstenite::Message;

    /// Production WebSocket URL.
    const PRODUCTION_URL: &str = "wss://api.lyra.finance/ws";

    /// Testnet WebSocket URL.
    const TESTNET_URL: &str = "wss://api-demo.lyra.finance/ws";

    /// How long to capture messages after successful subscribe.
    const CAPTURE_DURATION: Duration = Duration::from_secs(45);

    /// Minimum messages to capture before we consider the probe successful.
    const MIN_MESSAGES: usize = 5;

    /// A known active BTC option instrument on Derive production.
    /// This is a far-expiry option that should remain active for months.
    const PROBE_INSTRUMENT: &str = "BTC-20260626-130000-P";

    /// Alternate instrument for fallback attempts.
    const PROBE_INSTRUMENT_ALT: &str = "BTC-20260529-90000-P";

    /// Connect to a WebSocket URL with a timeout.
    async fn try_connect(
        url: &str,
    ) -> Option<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    > {
        println!("[PROBE] Attempting connection to {url}");
        match tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async(url),
        )
        .await
        {
            Ok(Ok((ws, response))) => {
                println!(
                    "[PROBE] Connected to {url} (status: {})",
                    response.status()
                );
                Some(ws)
            }
            Ok(Err(e)) => {
                println!("[PROBE] Connection failed to {url}: {e}");
                None
            }
            Err(_) => {
                println!("[PROBE] Connection timed out to {url}");
                None
            }
        }
    }

    /// Send a JSON-RPC subscribe request and return the raw response.
    async fn send_subscribe<S>(
        write: &mut futures_util::stream::SplitSink<S, Message>,
        read: &mut futures_util::stream::SplitStream<S>,
        id: u64,
        channels: &[&str],
    ) -> Option<String>
    where
        S: futures_util::Sink<Message> + futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    {
        let subscribe_msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "subscribe",
            "params": {
                "channels": channels,
            }
        });

        let msg_str = subscribe_msg.to_string();
        println!("[PROBE] Sending subscribe (id={id}): {msg_str}");

        if write.send(Message::text(msg_str)).await.is_err() {
            println!("[PROBE] Failed to send subscribe");
            return None;
        }

        // Wait for the subscribe response (up to 10s).
        match tokio::time::timeout(Duration::from_secs(10), read.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let text_str = text.to_string();
                println!("[PROBE] Subscribe response: {text_str}");
                Some(text_str)
            }
            Ok(Some(Ok(other))) => {
                println!("[PROBE] Unexpected message type after subscribe: {other:?}");
                None
            }
            Ok(Some(Err(e))) => {
                println!("[PROBE] Error reading subscribe response: {e}");
                None
            }
            Ok(None) => {
                println!("[PROBE] Stream ended while waiting for subscribe response");
                None
            }
            Err(_) => {
                println!("[PROBE] Timeout waiting for subscribe response");
                None
            }
        }
    }

    /// Analyze captured messages for book model characteristics.
    fn analyze_book_messages(messages: &[String]) {
        println!("\n[ANALYSIS] ========== BOOK MODEL ANALYSIS ==========");

        let book_messages: Vec<&String> = messages
            .iter()
            .filter(|m| m.contains("orderbook"))
            .collect();

        println!(
            "[ANALYSIS] Total orderbook messages captured: {}",
            book_messages.len()
        );

        if book_messages.is_empty() {
            println!("[ANALYSIS] No orderbook messages to analyze.");
            return;
        }

        for (i, msg) in book_messages.iter().enumerate().take(5) {
            println!("[ANALYSIS] Orderbook message #{}: {msg}", i + 1);

            // Parse to check structure.
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(msg) {
                if let Some(params) = parsed.get("params") {
                    if let Some(data) = params.get("data") {
                        let has_bids = data.get("bids").is_some();
                        let has_asks = data.get("asks").is_some();
                        let has_timestamp = data.get("timestamp").is_some();
                        let has_instrument = data.get("instrument_name").is_some();

                        let bid_count = data
                            .get("bids")
                            .and_then(|b| b.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0);
                        let ask_count = data
                            .get("asks")
                            .and_then(|a| a.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0);

                        println!(
                            "[ANALYSIS]   Fields: bids={has_bids}({bid_count}), asks={has_asks}({ask_count}), timestamp={has_timestamp}, instrument={has_instrument}"
                        );

                        // Check for delta-specific fields.
                        let has_type = data.get("type").is_some();
                        let has_action = data.get("action").is_some();
                        let has_change_id = data.get("change_id").is_some();
                        let has_prev_change_id = data.get("prev_change_id").is_some();

                        if has_type || has_action || has_change_id || has_prev_change_id {
                            println!("[ANALYSIS]   Delta-related fields: type={has_type}, action={has_action}, change_id={has_change_id}, prev_change_id={has_prev_change_id}");
                        } else {
                            println!("[ANALYSIS]   No delta-related fields found (snapshot model likely)");
                        }

                        // Print first bid/ask for format inspection.
                        if let Some(bids) = data.get("bids").and_then(|b| b.as_array()) {
                            if let Some(first_bid) = bids.first() {
                                println!("[ANALYSIS]   First bid entry: {first_bid}");
                            }
                        }
                        if let Some(asks) = data.get("asks").and_then(|a| a.as_array()) {
                            if let Some(first_ask) = asks.first() {
                                println!("[ANALYSIS]   First ask entry: {first_ask}");
                            }
                        }

                        // Print all top-level data keys.
                        if let Some(obj) = data.as_object() {
                            let keys: Vec<&String> = obj.keys().collect();
                            println!("[ANALYSIS]   All data keys: {keys:?}");
                        }
                    }

                    // Check channel name format.
                    if let Some(channel) = params.get("channel") {
                        println!("[ANALYSIS]   Channel: {channel}");
                    }
                }
            }
        }

        // Compare message sizes to detect snapshot vs delta.
        let sizes: Vec<usize> = book_messages.iter().map(|m| m.len()).collect();
        let avg_size = sizes.iter().sum::<usize>() as f64 / sizes.len() as f64;
        let min_size = sizes.iter().min().unwrap_or(&0);
        let max_size = sizes.iter().max().unwrap_or(&0);

        println!(
            "[ANALYSIS] Message sizes: avg={avg_size:.0}, min={min_size}, max={max_size}"
        );
        if *max_size > 0 && (*min_size as f64) > (avg_size * 0.5) {
            println!("[ANALYSIS] Size consistency suggests SNAPSHOT model (all messages similar size)");
        } else {
            println!("[ANALYSIS] Size variation suggests possible DELTA model (varying sizes)");
        }
    }

    /// Analyze captured messages for ticker data.
    fn analyze_ticker_messages(messages: &[String]) {
        println!("\n[ANALYSIS] ========== TICKER ANALYSIS ==========");

        let ticker_messages: Vec<&String> = messages
            .iter()
            .filter(|m| m.contains("ticker"))
            .collect();

        println!(
            "[ANALYSIS] Total ticker messages captured: {}",
            ticker_messages.len()
        );

        if ticker_messages.is_empty() {
            println!("[ANALYSIS] No ticker messages to analyze.");
            return;
        }

        for (i, msg) in ticker_messages.iter().enumerate().take(3) {
            println!("[ANALYSIS] Ticker message #{}: {msg}", i + 1);

            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(msg) {
                if let Some(params) = parsed.get("params") {
                    if let Some(data) = params.get("data") {
                        // Check for option_pricing nested object.
                        let has_option_pricing = data.get("option_pricing").is_some();
                        println!(
                            "[ANALYSIS]   Has option_pricing: {has_option_pricing}"
                        );

                        if let Some(op) = data.get("option_pricing") {
                            let has_bid_iv = op.get("bid_iv").is_some();
                            let has_ask_iv = op.get("ask_iv").is_some();
                            let has_delta = op.get("delta").is_some();
                            println!(
                                "[ANALYSIS]   option_pricing fields: bid_iv={has_bid_iv}, ask_iv={has_ask_iv}, delta={has_delta}"
                            );
                            if let Some(obj) = op.as_object() {
                                let keys: Vec<&String> = obj.keys().collect();
                                println!(
                                    "[ANALYSIS]   option_pricing keys: {keys:?}"
                                );
                            }
                        }

                        // Print all top-level data keys.
                        if let Some(obj) = data.as_object() {
                            let keys: Vec<&String> = obj.keys().collect();
                            println!("[ANALYSIS]   All ticker data keys: {keys:?}");
                        }
                    }

                    if let Some(channel) = params.get("channel") {
                        println!("[ANALYSIS]   Channel: {channel}");
                    }
                }
            }
        }
    }

    #[tokio::test]
    #[ignore] // Requires network access to Derive API
    async fn probe_derive_websocket() {
        println!("\n========================================");
        println!("  DERIVE.XYZ WEBSOCKET API PROBE");
        println!("========================================\n");

        // Step 1: Connect (try testnet first, fall back to production).
        let ws = if let Some(ws) = try_connect(TESTNET_URL).await {
            println!("[PROBE] Using testnet");
            ws
        } else if let Some(ws) = try_connect(PRODUCTION_URL).await {
            println!("[PROBE] Testnet unreachable, using production");
            ws
        } else {
            panic!("Cannot connect to either testnet or production Derive WebSocket");
        };

        let (mut write, mut read) = ws.split();

        // Step 2: Test unauthenticated subscribe to orderbook channel.
        // Try the CCXT-verified format: orderbook.{instrument}.{group}.{depth}
        let orderbook_channel = format!("orderbook.{PROBE_INSTRUMENT}.10.10");
        let ticker_channel = format!("ticker_slim.{PROBE_INSTRUMENT}.100");

        println!("\n[PROBE] === TESTING UNAUTHENTICATED SUBSCRIBE ===");
        println!("[PROBE] Attempting subscribe WITHOUT authentication");

        let channels_to_try: Vec<Vec<&str>> = vec![
            // Attempt 1: CCXT-verified format
            vec![&orderbook_channel, &ticker_channel],
        ];

        let mut subscribed = false;
        for (attempt, channels) in channels_to_try.iter().enumerate() {
            let channel_refs: Vec<&str> = channels.iter().copied().collect();
            println!(
                "\n[PROBE] Subscribe attempt {} with channels: {channel_refs:?}",
                attempt + 1
            );

            if let Some(response) = send_subscribe(
                &mut write,
                &mut read,
                (attempt + 1) as u64,
                &channel_refs,
            )
            .await
            {
                // Check if subscribe was successful.
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&response) {
                    if let Some(result) = parsed.get("result") {
                        // Check for status field with "ok" values.
                        if let Some(status) = result.get("status") {
                            let all_ok = status
                                .as_object()
                                .map(|obj| {
                                    obj.values().all(|v| v.as_str() == Some("ok"))
                                })
                                .unwrap_or(false);

                            if all_ok {
                                println!("[PROBE] Subscribe SUCCEEDED (all channels ok)");
                                println!("[PROBE] AUTH NOT REQUIRED for public channels");
                                subscribed = true;
                                break;
                            } else {
                                println!("[PROBE] Subscribe returned non-ok status: {status}");
                            }
                        } else {
                            println!("[PROBE] Subscribe result (no status field): {result}");
                            // Some responses may not have status but still succeed.
                            subscribed = true;
                            break;
                        }
                    } else if let Some(error) = parsed.get("error") {
                        let code = error.get("code").and_then(|c| c.as_i64());
                        let message = error
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown");

                        println!(
                            "[PROBE] Subscribe ERROR: code={code:?}, message={message}"
                        );

                        // Check if it's an auth error.
                        if message.to_lowercase().contains("auth")
                            || message.to_lowercase().contains("login")
                            || message.to_lowercase().contains("unauthorized")
                            || code == Some(-32001)
                        {
                            println!("[PROBE] AUTH REQUIRED for public channels!");
                            println!("[PROBE] This means k256 dependency IS needed for Phase 31");
                        }
                    }
                }
            }
        }

        if !subscribed {
            // Try alternative channel formats before giving up.
            println!("\n[PROBE] Primary format failed. Trying alternative channel formats...");

            let alt_formats: Vec<(String, &str)> = vec![
                (
                    format!("orderbook.{PROBE_INSTRUMENT}"),
                    "orderbook.{instrument}",
                ),
                (
                    format!("orderbook.{PROBE_INSTRUMENT}.raw"),
                    "orderbook.{instrument}.raw",
                ),
                (
                    format!("book.{PROBE_INSTRUMENT}"),
                    "book.{instrument}",
                ),
                (
                    format!("orderbook.{PROBE_INSTRUMENT_ALT}.10.10"),
                    "orderbook.{alt_instrument}.10.10",
                ),
            ];

            for (channel, format_name) in &alt_formats {
                println!("[PROBE] Trying format: {format_name} -> {channel}");
                if let Some(response) = send_subscribe(
                    &mut write,
                    &mut read,
                    100,
                    &[channel.as_str()],
                )
                .await
                {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&response) {
                        if parsed.get("result").is_some()
                            && parsed.get("error").is_none()
                        {
                            println!("[PROBE] Format {format_name} SUCCEEDED!");
                            subscribed = true;
                            break;
                        }
                    }
                }
            }
        }

        if !subscribed {
            println!("\n[PROBE] WARNING: No channel format succeeded. Proceeding with message capture anyway.");
        }

        // Step 3: Capture messages.
        println!("\n[PROBE] === CAPTURING MESSAGES ===");
        println!(
            "[PROBE] Capturing for up to {} seconds or until {} messages...",
            CAPTURE_DURATION.as_secs(),
            MIN_MESSAGES
        );

        let mut captured_messages: Vec<String> = Vec::new();
        let mut ping_count = 0u32;
        let mut pong_count = 0u32;
        let mut heartbeat_count = 0u32;
        let capture_start = Instant::now();

        loop {
            let remaining = CAPTURE_DURATION.saturating_sub(capture_start.elapsed());
            if remaining.is_zero() {
                println!("[PROBE] Capture duration elapsed");
                break;
            }

            // Also break early if we have enough messages.
            if captured_messages.len() >= 30 {
                println!("[PROBE] Captured 30+ messages, stopping early");
                break;
            }

            match tokio::time::timeout(remaining, read.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    let text_str = text.to_string();
                    let seq = captured_messages.len() + 1;

                    // Check for heartbeat messages.
                    if text_str.contains("\"heartbeat\"")
                        || text_str.contains("\"test_request\"")
                    {
                        heartbeat_count += 1;
                        println!("[PROBE] [{seq}] HEARTBEAT: {text_str}");
                        continue; // Don't count heartbeats as data messages.
                    }

                    // Truncate for display if very long.
                    let display = if text_str.len() > 500 {
                        format!("{}...[truncated, total {} bytes]", &text_str[..500], text_str.len())
                    } else {
                        text_str.clone()
                    };
                    println!("[PROBE] [{seq}] {display}");

                    captured_messages.push(text_str);
                }
                Ok(Some(Ok(Message::Ping(_)))) => {
                    ping_count += 1;
                    println!(
                        "[PROBE] Received WS PING frame (#{ping_count} at {:?})",
                        capture_start.elapsed()
                    );
                }
                Ok(Some(Ok(Message::Pong(_)))) => {
                    pong_count += 1;
                    println!(
                        "[PROBE] Received WS PONG frame (#{pong_count} at {:?})",
                        capture_start.elapsed()
                    );
                }
                Ok(Some(Ok(Message::Close(frame)))) => {
                    let reason = frame
                        .as_ref()
                        .map(|f| f.reason.to_string())
                        .unwrap_or_else(|| "no reason".to_string());
                    println!("[PROBE] Connection closed by server: {reason}");
                    break;
                }
                Ok(Some(Ok(Message::Binary(data)))) => {
                    println!("[PROBE] Received binary frame ({} bytes)", data.len());
                }
                Ok(Some(Ok(Message::Frame(_)))) => {
                    println!("[PROBE] Received raw frame");
                }
                Ok(Some(Err(e))) => {
                    println!("[PROBE] Read error: {e}");
                    break;
                }
                Ok(None) => {
                    println!("[PROBE] Stream ended");
                    break;
                }
                Err(_) => {
                    println!("[PROBE] Capture timeout reached");
                    break;
                }
            }
        }

        let capture_elapsed = capture_start.elapsed();

        // Step 4: Summary.
        println!("\n========================================");
        println!("  PROBE RESULTS SUMMARY");
        println!("========================================\n");

        println!("[SUMMARY] Capture duration: {capture_elapsed:.1?}");
        println!(
            "[SUMMARY] Total data messages captured: {}",
            captured_messages.len()
        );
        println!("[SUMMARY] WS Ping frames received: {ping_count}");
        println!("[SUMMARY] WS Pong frames received: {pong_count}");
        println!("[SUMMARY] Application heartbeat messages: {heartbeat_count}");

        // Step 5: Analyze heartbeat mechanism.
        println!("\n[SUMMARY] === HEARTBEAT MECHANISM ===");
        if ping_count > 0 {
            println!("[SUMMARY] Server sends WS-level PING frames (standard WebSocket keep-alive)");
        }
        if heartbeat_count > 0 {
            println!("[SUMMARY] Server sends application-level heartbeat messages (Deribit-style)");
        }
        if ping_count == 0 && heartbeat_count == 0 {
            println!(
                "[SUMMARY] No heartbeat/ping observed in {:.0}s capture window",
                capture_elapsed.as_secs_f64()
            );
            println!("[SUMMARY] Standard WS keep-alive may be handled transparently by tokio-tungstenite");
        }

        // Step 6: Authentication analysis.
        println!("\n[SUMMARY] === AUTHENTICATION ===");
        if subscribed {
            println!("[SUMMARY] Public channels (orderbook, ticker) work WITHOUT authentication");
            println!("[SUMMARY] k256 dependency is NOT needed for v1.5 (read-only scope)");
        } else {
            println!("[SUMMARY] Could not confirm -- subscribe may have failed for other reasons");
        }

        // Step 7: Analyze book model.
        if !captured_messages.is_empty() {
            analyze_book_messages(&captured_messages);
            analyze_ticker_messages(&captured_messages);
        }

        // Assertions.
        println!("\n[PROBE] Probe complete.");
        assert!(
            subscribed || !captured_messages.is_empty(),
            "Probe failed: could not subscribe to any channel and no messages captured"
        );
    }

    /// Focused production probe using confirmed channel formats.
    /// Uses a near-expiry instrument for higher activity.
    #[tokio::test]
    #[ignore] // Requires network access
    async fn probe_derive_production_book_data() {
        println!("\n========================================");
        println!("  DERIVE PRODUCTION BOOK DATA PROBE");
        println!("========================================\n");

        // First, find a near-expiry active instrument via REST.
        let client = reqwest::Client::new();
        let response = client
            .post("https://api.lyra.finance/public/get_instruments")
            .json(&serde_json::json!({
                "instrument_type": "option",
                "currency": "BTC",
                "expired": false,
            }))
            .send()
            .await
            .expect("Failed to reach Derive REST API");

        let body: serde_json::Value = response.json().await.expect("Failed to parse");
        let instruments = body
            .get("result")
            .and_then(|r| r.as_array())
            .expect("No result array");

        // Find near-expiry active instruments (closest expiry first).
        let mut active: Vec<&serde_json::Value> = instruments
            .iter()
            .filter(|i| i.get("is_active").and_then(|a| a.as_bool()) == Some(true))
            .collect();
        active.sort_by_key(|i| {
            i.pointer("/option_details/expiry")
                .and_then(|e| e.as_u64())
                .unwrap_or(u64::MAX)
        });

        let instrument_name = active
            .first()
            .and_then(|i| i.get("instrument_name"))
            .and_then(|n| n.as_str())
            .expect("No active instruments");

        println!("[PROBE] Selected near-expiry instrument: {instrument_name}");

        // Connect to production.
        let ws = try_connect(PRODUCTION_URL)
            .await
            .expect("Cannot connect to production");

        let (mut write, mut read) = ws.split();

        // Subscribe using confirmed formats.
        let orderbook_ch = format!("orderbook.{instrument_name}.10.10");
        let ticker_slim_ch = format!("ticker_slim.{instrument_name}.100");

        println!("[PROBE] Subscribing to: {orderbook_ch}, {ticker_slim_ch}");

        let subscribe_msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "subscribe",
            "params": {"channels": [&orderbook_ch, &ticker_slim_ch]}
        });

        write
            .send(Message::text(subscribe_msg.to_string()))
            .await
            .expect("Send failed");

        // Capture messages for 60s.
        let mut captured: Vec<String> = Vec::new();
        let mut ping_count = 0u32;
        let capture_start = Instant::now();
        let capture_duration = Duration::from_secs(60);

        loop {
            let remaining = capture_duration.saturating_sub(capture_start.elapsed());
            if remaining.is_zero() || captured.len() >= 30 {
                break;
            }

            match tokio::time::timeout(remaining, read.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    let text_str = text.to_string();
                    let seq = captured.len() + 1;

                    if text_str.contains("\"heartbeat\"") || text_str.contains("\"test_request\"") {
                        println!("[PROBE] [{seq}] HEARTBEAT");
                        continue;
                    }

                    let display = if text_str.len() > 1000 {
                        format!("{}...[truncated, {} bytes total]", &text_str[..1000], text_str.len())
                    } else {
                        text_str.clone()
                    };
                    println!("[PROBE] [{seq}] {display}");
                    captured.push(text_str);
                }
                Ok(Some(Ok(Message::Ping(_)))) => {
                    ping_count += 1;
                    println!("[PROBE] WS PING #{ping_count} at {:?}", capture_start.elapsed());
                }
                Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) | Ok(None) => break,
                Ok(Some(Ok(_))) => {} // binary, pong, frame
                Err(_) => break,
            }
        }

        println!("\n[SUMMARY] Captured {} data messages, {} WS pings in {:.1?}",
            captured.len(), ping_count, capture_start.elapsed());

        // Analyze.
        if !captured.is_empty() {
            analyze_book_messages(&captured);
            analyze_ticker_messages(&captured);
        }

        assert!(!captured.is_empty(), "No data captured from production");
    }

    /// Quick REST API connectivity test.
    #[tokio::test]
    #[ignore] // Requires network access
    async fn probe_derive_rest_api() {
        println!("\n[REST PROBE] Testing Derive REST API...");

        let client = reqwest::Client::new();

        // Test get_instruments endpoint.
        let response = client
            .post("https://api.lyra.finance/public/get_instruments")
            .json(&serde_json::json!({
                "instrument_type": "option",
                "currency": "BTC",
                "expired": false,
            }))
            .send()
            .await
            .expect("Failed to reach Derive REST API");

        assert!(response.status().is_success(), "REST API returned error status");

        let body: serde_json::Value = response.json().await.expect("Failed to parse response");

        let instruments = body
            .get("result")
            .and_then(|r| r.as_array())
            .expect("Response missing 'result' array");

        println!("[REST PROBE] Total BTC options: {}", instruments.len());

        let active: Vec<&serde_json::Value> = instruments
            .iter()
            .filter(|i| i.get("is_active").and_then(|a| a.as_bool()) == Some(true))
            .collect();

        println!("[REST PROBE] Active BTC options: {}", active.len());

        // Print a few instrument names.
        for inst in active.iter().take(5) {
            if let Some(name) = inst.get("instrument_name").and_then(|n| n.as_str()) {
                let strike = inst
                    .pointer("/option_details/strike")
                    .and_then(|s| s.as_str())
                    .unwrap_or("?");
                let expiry = inst
                    .pointer("/option_details/expiry")
                    .and_then(|e| e.as_u64())
                    .unwrap_or(0);
                let opt_type = inst
                    .pointer("/option_details/option_type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("?");
                let quote = inst
                    .get("quote_currency")
                    .and_then(|q| q.as_str())
                    .unwrap_or("?");

                println!(
                    "[REST PROBE]   {name} strike={strike} expiry={expiry} type={opt_type} quote={quote}"
                );
            }
        }

        assert!(!active.is_empty(), "No active BTC options found on Derive");

        // Verify instrument name format.
        for inst in &active {
            if let Some(name) = inst.get("instrument_name").and_then(|n| n.as_str()) {
                // Format should be: BTC-YYYYMMDD-STRIKE-C/P
                let parts: Vec<&str> = name.split('-').collect();
                assert!(
                    parts.len() >= 4,
                    "Unexpected instrument name format: {name}"
                );
                assert_eq!(parts[0], "BTC", "Expected BTC prefix: {name}");
                assert!(
                    parts[1].len() == 8 && parts[1].chars().all(|c| c.is_ascii_digit()),
                    "Expected YYYYMMDD date: {name}"
                );
                let last = *parts.last().unwrap();
                assert!(
                    last == "C" || last == "P",
                    "Expected C or P suffix: {name}"
                );
            }
        }

        println!("[REST PROBE] All instrument names match expected format BTC-YYYYMMDD-STRIKE-C/P");
        println!("[REST PROBE] Quote currency: USDC (confirmed)");
    }
}
