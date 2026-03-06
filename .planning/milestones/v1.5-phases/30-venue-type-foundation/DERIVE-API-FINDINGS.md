# Derive.xyz API Findings

> Live API probe conducted 2026-03-04 against `wss://api.lyra.finance/ws` (production).
> 30 messages captured in 7 seconds. All findings are from direct observation.

## 1. Channel Subscription Format

**Confidence: CONFIRMED** (live capture from production)

### Subscribe Method

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "subscribe",
  "params": {
    "channels": [
      "orderbook.BTC-20260305-69500-P.10.10",
      "ticker_slim.BTC-20260305-69500-P.100"
    ]
  }
}
```

### Subscribe Response

```json
{
  "id": 1,
  "result": {
    "status": {
      "orderbook.BTC-20260305-69500-P.10.10": "ok",
      "ticker_slim.BTC-20260305-69500-P.100": "ok"
    },
    "current_subscriptions": [
      "ticker_slim.BTC-20260305-69500-P.100",
      "orderbook.BTC-20260305-69500-P.10.10"
    ]
  }
}
```

### Channel Format Details

| Channel Type | Format | Example |
|---|---|---|
| Orderbook | `orderbook.{instrument_name}.{group}.{depth}` | `orderbook.BTC-20260305-69500-P.10.10` |
| Ticker | `ticker_slim.{instrument_name}.{interval_ms}` | `ticker_slim.BTC-20260305-69500-P.100` |

**CRITICAL:** The `ticker` channel is **deprecated**. The API returns an explicit error:
```
`ticker` channel has been deprecated. Please use `ticker_slim`.
```

### Instrument Name Format

Confirmed via REST API (`/public/get_instruments`): `{ASSET}-{YYYYMMDD}-{STRIKE}-{C|P}`

- 702 active BTC options on production
- Quote currency: USDC (confirmed from REST response)
- Example: `BTC-20260305-69500-P`, `BTC-20260308-71000-C`

## 2. Book Update Model

**Confidence: CONFIRMED** (23 orderbook messages analyzed from live capture)

### Model: SNAPSHOT-ONLY

Every orderbook message contains the **full current book state** (all bids and asks up to the requested depth). There are no delta/incremental update messages.

Evidence:
- No `type`, `action`, `change_id`, or `prev_change_id` fields in any message
- All messages have identical structure with complete `bids` and `asks` arrays
- Message sizes are consistent (222-286 bytes, avg 276 bytes) -- no small deltas
- Each message has a monotonically increasing `publish_id` (56593, 56594, 56595, ...)

### Sample Orderbook Message

```json
{
  "method": "subscription",
  "params": {
    "channel": "orderbook.BTC-20260305-69500-P.10.10",
    "data": {
      "timestamp": 1772624842966,
      "instrument_name": "BTC-20260305-69500-P",
      "publish_id": 56593,
      "bids": [["340", "0.4"], ["320", "1"], ["280", "0.70343"]],
      "asks": [["420", "0.4"], ["520", "0.70343"]]
    }
  }
}
```

### Orderbook Data Fields

| Field | Type | Description |
|---|---|---|
| `timestamp` | integer (ms) | Server timestamp in milliseconds |
| `instrument_name` | string | Instrument identifier |
| `publish_id` | integer | Monotonically increasing sequence number |
| `bids` | `[[price, amount], ...]` | Bid levels (strings, not numbers) |
| `asks` | `[[price, amount], ...]` | Ask levels (strings, not numbers) |

**Implementation note:** Prices and amounts are **strings**, not numbers. Parser must convert.

### Update Frequency

Orderbook snapshots arrive approximately every 100ms (observed 10 messages per second). This is much more frequent than Deribit and simplifies the feed -- no need for snapshot+delta reconciliation logic.

## 3. Heartbeat Mechanism

**Confidence: CONFIRMED** (observed from both testnet and production)

### Mechanism: Standard WebSocket PING/PONG

- Server sends WS-level **PING frames** at approximately 30-second intervals
- `tokio-tungstenite` handles PONG responses automatically (no application code needed)
- No application-level heartbeat protocol (unlike Deribit's `set_heartbeat`/`test_request` system)

Evidence:
- Testnet: 1 WS PING received at 29.3s during 45s capture window
- Production: 0 PINGs during 7s capture (expected -- interval is ~30s)
- No JSON-RPC heartbeat notifications observed in any captured messages

### Implementation Impact

Unlike the Deribit client which requires explicit `public/set_heartbeat` and `public/test` response handling, the Derive client only needs:
- A **dead connection timeout** (e.g., 60s with no messages or pings)
- No heartbeat setup request
- No heartbeat response handler

This significantly simplifies the client compared to `src/feed/deribit/client.rs`.

## 4. Authentication for Public Channels

**Confidence: CONFIRMED** (live subscribe without any auth)

### Result: NO AUTHENTICATION REQUIRED for public channels

Both `orderbook.*` and `ticker_slim.*` channels accept subscriptions without prior authentication. The subscribe response returns `"status": {"channel": "ok"}` immediately.

### Implementation Impact

- **k256 dependency is NOT needed for v1.5** (read-only market data scope)
- No Ethereum wallet signing required for subscribing to orderbook and ticker data
- Authentication can be deferred to v2 if private channels (orders, positions) are needed

### Error Codes Observed

| Code | Message | When |
|---|---|---|
| `-32602` | "Invalid params" | Using deprecated `ticker` channel name |
| `13000` | "Invalid channels" | Using wrong channel format (e.g., `orderbook.{inst}` without group/depth) |

## 5. Ticker Slim Data Structure

**Confidence: CONFIRMED** (7 ticker_slim messages analyzed)

### Sample Ticker Slim Message

```json
{
  "method": "subscription",
  "params": {
    "channel": "ticker_slim.BTC-20260305-69500-P.100",
    "data": {
      "timestamp": 1772624842966,
      "instrument_ticker": {
        "t": 1772624842966,
        "A": "0.4",
        "a": "414",
        "B": "0.4",
        "b": "341",
        "f": null,
        "option_pricing": {
          "d": "-0.24967",
          "t": "-453.85103",
          "g": "0.00013192",
          "v": "10.84014",
          "i": "0.70513",
          "r": "0.84114",
          "f": "71067",
          "m": "364",
          "df": "1",
          "bi": "0.68323",
          "ai": "0.75013"
        },
        "I": "71078",
        "M": "364",
        "stats": {
          "c": "1.3",
          "v": "91411.632",
          "pr": "787.353",
          "n": 2,
          "oi": "1.3",
          "h": "943.464",
          "l": "504.314",
          "p": "-0.465"
        },
        "minp": "4",
        "maxp": "1968"
      }
    }
  }
}
```

### Ticker Slim Fields (Key for Phase 31)

| Field | Key | Meaning | Value Example |
|---|---|---|---|
| Best ask size | `A` | Best ask amount | "0.4" |
| Best ask price | `a` | Best ask price (USDC) | "414" |
| Best bid size | `B` | Best bid amount | "0.4" |
| Best bid price | `b` | Best bid price (USDC) | "341" |
| Index price | `I` | Underlying index price | "71078" |
| Mark price | `M` | Mark price | "364" |
| Delta | `option_pricing.d` | Option delta | "-0.24967" |
| Theta | `option_pricing.t` | Option theta | "-453.85103" |
| Gamma | `option_pricing.g` | Option gamma | "0.00013192" |
| Vega | `option_pricing.v` | Option vega | "10.84014" |
| IV (mid) | `option_pricing.i` | Implied volatility (mid) | "0.70513" |
| IV (bid) | `option_pricing.bi` | Implied volatility (bid) | "0.68323" |
| IV (ask) | `option_pricing.ai` | Implied volatility (ask) | "0.75013" |
| Forward | `option_pricing.f` | Forward price | "71067" |
| Rate | `option_pricing.r` | Risk-free rate | "0.84114" |

**All values are strings.** The `ticker_slim` format uses abbreviated single-letter keys (unlike the deprecated `ticker` which used full names).

### Update Frequency

Ticker updates arrive approximately every 1 second (interval parameter `100` = 100ms minimum, but actual frequency is ~1/s).

## Summary for Phase 31 Implementation

| Question | Answer | Confidence |
|---|---|---|
| Channel subscribe format | `orderbook.{inst}.{group}.{depth}`, `ticker_slim.{inst}.{interval}` | CONFIRMED |
| Book update model | Snapshot-only (no deltas) | CONFIRMED |
| Heartbeat mechanism | WS-level PING/PONG (auto-handled by tokio-tungstenite) | CONFIRMED |
| Auth for public channels | Not required | CONFIRMED |
| Prices format | Strings (must parse to Decimal) | CONFIRMED |
| Quote currency | USDC | CONFIRMED |
| `ticker` vs `ticker_slim` | `ticker` is deprecated, must use `ticker_slim` | CONFIRMED |
| k256 dependency needed? | No (not for v1.5 read-only scope) | CONFIRMED |

### Key Differences from Deribit

| Aspect | Deribit | Derive |
|---|---|---|
| Book model | Snapshot + Delta | Snapshot only |
| Heartbeat | Application-level (`set_heartbeat`/`test_request`) | WS PING/PONG |
| Subscribe method | `public/subscribe` | `subscribe` |
| Ticker channel | `ticker.{inst}.{interval}` | `ticker_slim.{inst}.{interval}` |
| Price format | Numeric (floats) | Strings |
| Quote currency | BTC (inverse) | USDC (linear) |
| Auth for public | Not required | Not required |
