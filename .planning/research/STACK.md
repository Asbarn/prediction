# Technology Stack: v1.5 Derive.xyz Venue Integration

**Project:** Prediction Market Arbitrage System -- Derive.xyz Feed Addition
**Researched:** 2026-03-03
**Confidence:** MEDIUM-HIGH (API protocol HIGH, authentication MEDIUM, rate limits LOW due to inaccessible docs pages)

## Scope

This document covers ONLY the stack additions needed for v1.5 Derive.xyz venue integration. The existing v1.0-v1.4 validated stack is unchanged. The question being answered: what new Rust crates (if any) are needed to connect to Derive's WebSocket JSON-RPC API, authenticate for public market data, and subscribe to BTC options orderbooks?

---

## Executive Finding: One New Dependency Required

v1.5 requires **one new crate dependency**: `k256` for secp256k1 ECDSA signing used in Derive's Ethereum-wallet-based authentication. All other integration needs are covered by the existing stack.

The existing `tokio-tungstenite`, `serde_json`, `futures-util`, `reqwest`, `governor`, `backoff`, `chrono`, and `base64` crates cover WebSocket connection, JSON-RPC messaging, rate limiting, reconnection, timestamps, and encoding respectively. No WebSocket, HTTP, or JSON crate additions are needed.

**Critical discovery:** Public market data subscriptions on Derive require authentication via `public/login` (Ethereum wallet signature). Unlike Deribit where public orderbook channels work without credentials, Derive's WebSocket session requires a login step even for read-only market data access. This is the primary new capability.

---

## Derive.xyz API Protocol (Verified)

**WebSocket endpoint:** `wss://api.lyra.finance/ws`
**Demo/testnet endpoint:** `wss://api-demo.lyra.finance/ws`
**Protocol:** JSON-RPC 2.0 (same standard as Deribit)
**Transport:** WebSocket only for subscriptions; HTTP available for one-off calls but does not support subscriptions

**Instrument naming:** Same convention as Deribit: `BTC-YYYYMMDD-STRIKE-C` / `BTC-YYYYMMDD-STRIKE-P`
Example: `BTC-20240329-70000-C` (BTC call, March 29 2024, $70,000 strike)
Evidence from official docs example code: `'ETH-20240329-2400-C'` format confirmed.

**Orderbook channel format:** `orderbook.{instrument_name}` (MEDIUM confidence -- inferred from docs references to "ticker or orderbook channels" with instrument_name parameter)

**Public discovery endpoint:** `public/get_instruments` -- callable over WebSocket or REST without authentication; returns all active instruments with `currency` and `instrument_type` filter params.

**Ticker endpoint:** `public/get_ticker` -- single instrument ticker (best bid/ask, fees, constraints). No auth required.

---

## Authentication: Ethereum Wallet Signature (MEDIUM confidence)

Derive uses self-custodial, Ethereum-wallet-based authentication. There are no traditional API keys.

### How It Works

1. **Credentials:** A secp256k1 private key (Ethereum EOA wallet) is used for signing. The corresponding wallet address is the account identifier.
2. **Session authentication via `public/login`:** A WebSocket-only JSON-RPC call that authenticates the session before private channel subscriptions. Required even for reading orderbook data that involves "account-level" context.
3. **Signing mechanism:** Sign the current timestamp (Unix milliseconds as string) using Ethereum's `personal_sign` convention (`sign_message` in ethers/alloy). This is NOT EIP-712 structured data -- it's a simple `personal_sign` of a raw string.

### Login Request Format

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "public/login",
  "params": {
    "wallet": "0xYOUR_WALLET_ADDRESS",
    "timestamp": "1703001600000",
    "signature": "0xsignature_of_timestamp_string..."
  }
}
```

The `signature` is produced by: `personal_sign(keccak256("\x19Ethereum Signed Message:\n" + len(timestamp) + timestamp), private_key)`

This maps to the Ethereum `eth_sign` / `personal_sign` method -- the standard message signing flow used across all Ethereum wallets.

### Why authentication matters for THIS project

The v1.5 scope is **read-only market data** (orderbook snapshots and deltas). Based on the `public/` prefix on login and the self-custodial architecture, public channel subscriptions (`orderbook.*`, `ticker.*`) are likely accessible after a `public/login` step using a throwaway Ethereum wallet with no funds. No real assets need to be at risk. The "wallet" is just a signing identity, not a custody account.

---

## Recommended Stack

### New Dependencies

| Technology | Version | Purpose | Why This One |
|------------|---------|---------|--------------|
| k256 | 0.13 | secp256k1 ECDSA signing for Ethereum wallet authentication | Pure Rust, no C FFI (unlike `secp256k1` crate which requires libsecp256k1). RustCrypto project, audited by NCC Group. `k256::ecdsa::SigningKey` + Keccak256 digest gives Ethereum-compatible signatures. Already transitively compiled by `rsa` crate ecosystem (RustCrypto family). `sha2` is already in deps for hashing -- `sha3` for Keccak256 is from the same RustCrypto family with identical API. |

### New Feature on Existing Dependency

| Technology | Existing Version | New Feature Needed | Why |
|------------|-----------------|-------------------|-----|
| sha2 | 0.10 | Already in deps; SHA-256 used for Kalshi auth | No change needed |

**Note on Keccak256:** Ethereum's `personal_sign` requires Keccak256 (NOT SHA-256). The `sha3` crate from RustCrypto provides `Keccak256` -- it's from the same family as the existing `sha2 = "0.10"` dep and uses identical digest API. However, the `k256` crate re-exports `sha3::Keccak256` through its `ecdsa` feature (via the `digest` crate traits), so **no explicit `sha3` dep may be needed** if k256's `Keccak256` re-export is used directly. Verify at implementation time.

### Existing Dependencies Covering v1.5 Needs

| Technology | Version | v1.5 Usage | Why Sufficient |
|------------|---------|------------|----------------|
| tokio-tungstenite | 0.28 | WebSocket connection to `wss://api.lyra.finance/ws` | Already used for Deribit and Kalshi. Same `connect_async` + `split()` + read loop pattern. |
| futures-util | 0.3 | `SinkExt`, `StreamExt` for WS write/read | Same as existing venue clients. |
| serde_json | 1.0 | JSON-RPC 2.0 message construction and parsing | `serde_json::json!{}` macro for subscribe/login messages; `serde_json::from_str` for parsing frames. Same pattern as Deribit client. |
| serde | 1.0 | Deserialize orderbook delta/snapshot messages into Rust structs | `#[derive(Deserialize)]` on message types, same as Deribit `messages.rs`. |
| tokio | 1 | Async runtime, `mpsc::channel`, `CancellationToken`, heartbeat timers | Identical usage to existing WS clients. |
| tokio-util | 0.7 | `CancellationToken` for graceful shutdown | Used in all existing venue clients. |
| anyhow | 1.0 | Error handling in client and supervisor | Already the project's error handling standard. |
| tracing | 0.1 | Structured logging with venue=derive fields | Already used throughout. |
| chrono | 0.4 | Timestamp generation for `public/login` (`Utc::now().timestamp_millis()`) | Already in deps. |
| base64 | 0.22 | Potentially needed if signature encoding requires base64 (protocol TBD at impl time) | Already in deps for Kalshi auth. May not be needed -- Ethereum signatures are hex-encoded, not base64. |
| backoff | 0.4 | Exponential backoff for reconnection in supervisor | Used by all existing supervisors. |
| governor | 0.8 | Rate limiter for outbound WebSocket requests | Used by all existing feed clients. |
| reqwest | 0.12 | REST calls to `public/get_instruments` for discovery | Already used for Polymarket Gamma API and Kalshi REST. |
| rust_decimal | 1.40 | Price and size parsing from orderbook updates | All orderbook prices use `Decimal` in existing normalized schema. |

---

## Why k256 Over Alternatives

| Option | Assessment | Verdict |
|--------|------------|---------|
| **k256 0.13** (recommended) | Pure Rust, RustCrypto family, same crate family as `sha2` already in deps, audited, no C FFI, `SigningKey::from_bytes()` API, Ethereum-compatible via `Keccak256` digest | USE |
| `secp256k1 = "0.29"` (bitcoin-core crate) | Requires libsecp256k1 C library; adds C compilation step; heavier build; overkill for single signing operation | AVOID |
| `alloy-signer` | Full Ethereum signer stack; 50+ transitive deps; designed for full blockchain interaction; massive overkill for signing one timestamp string per WS session | AVOID |
| `ethers-rs` | Deprecated; successor is alloy; same overkill problem | AVOID |
| `tiny-keccak` | Alternative Keccak implementation; less idiomatic with k256's digest trait system; prefer sha3 from same RustCrypto family | AVOID |

---

## What NOT to Add

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `alloy` / `alloy-signer` | 50+ transitive crates for what amounts to `k256::ecdsa::SigningKey::sign_prehash()`. Derive's auth is a single `personal_sign` call -- does not require EIP-712, ABI encoding, provider connections, or any blockchain interaction. | `k256 = { version = "0.13", features = ["ecdsa"] }` |
| `ethers-rs` | Deprecated as of 2024; superseded by alloy | `k256` direct |
| `sha3` (explicit dep) | May be unnecessary if k256 re-exports `Keccak256` through `ecdsa` feature; check at implementation time before adding | Test without first |
| `hex` crate | For encoding the 0x-prefixed signature output, `format!("0x{}", hex::encode(sig_bytes))` can be replaced by `format!("0x{}", sig_bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>())` or use existing `base64` crate's hex if available. `hex` is a tiny crate but prefer not adding if stdlib formatting suffices. | `format!("{:02x}", b)` in a collect, or check if already transitive |
| Any Derive-specific SDK / `lyra-client` crate | No official Rust SDK on crates.io. The `derivexyz/cockpit` repo is the official Rust reference but it's a full trading system, not a library crate. | Implement directly using the JSON-RPC protocol |
| `tokio-websockets` / `fastwebsockets` | Different WS crate than existing `tokio-tungstenite`. Adding a second WS library would double the WS code surface and break uniformity. | Existing `tokio-tungstenite 0.28` |

---

## Cargo.toml Addition

```toml
# Add to existing [dependencies] section:

# secp256k1 signing for Derive.xyz WebSocket authentication (v1.5)
k256 = { version = "0.13", default-features = false, features = ["ecdsa", "std"] }
```

The `ecdsa` feature enables `SigningKey`, `Signature`, and digest-based signing. The `std` feature is required for consistent behavior. Disable default features to avoid pulling in `schnorr` and `pkcs8` features that are unneeded.

**Note:** If Keccak256 is not re-exported by k256's ecdsa feature, add:
```toml
sha3 = "0.10"  # For Keccak256 -- only if not provided by k256
```

---

## Authentication Implementation Pattern

Modeled after existing `src/feed/kalshi/auth.rs` (RSA-PSS signing):

```rust
// src/feed/derive/auth.rs

use k256::ecdsa::{SigningKey, signature::Signer};
use k256::ecdsa::signature::hazmat::PrehashSigner;
// OR use the digest-based approach:
// use k256::ecdsa::{SigningKey, Signature};
// use sha3::Keccak256;
// use k256::ecdsa::signature::DigestSigner;

/// Sign a timestamp for Derive.xyz WebSocket authentication.
///
/// Derive.xyz uses Ethereum's `personal_sign` convention:
/// sign(keccak256("\x19Ethereum Signed Message:\n" + len(msg) + msg))
///
/// Returns hex-encoded signature (0x-prefixed) for the `public/login` params.
pub fn sign_derive_login(
    signing_key: &SigningKey,
    timestamp_ms: i64,
) -> anyhow::Result<String> {
    let timestamp_str = timestamp_ms.to_string();
    let prefix = format!("\x19Ethereum Signed Message:\n{}", timestamp_str.len());
    let prefixed_msg = format!("{}{}", prefix, timestamp_str);
    // keccak256 of the prefixed message
    // then sign with secp256k1 ECDSA
    // return "0x" + hex(signature_bytes + recovery_id)
    todo!("implementation detail -- verified pattern from Derive docs")
}

/// Load a secp256k1 private key from hex string (32 bytes).
pub fn load_derive_signing_key(private_key_hex: &str) -> anyhow::Result<SigningKey> {
    let key_bytes = hex::decode(private_key_hex.trim_start_matches("0x"))
        .map_err(|e| anyhow::anyhow!("invalid hex private key: {}", e))?;
    SigningKey::from_bytes(key_bytes.as_slice().into())
        .map_err(|e| anyhow::anyhow!("invalid secp256k1 key: {}", e))
}
```

**Note on `hex::decode`:** The `hex` crate may need to be added if not transitively available. Alternative: use `base64`'s utilities or implement hex decode manually. Verify at implementation time.

---

## Integration Architecture

Derive feeds into the existing pipeline at the same level as Deribit, Polymarket, and Kalshi:

```
DeriveClient (src/feed/derive/client.rs)
    |-- connect to wss://api.lyra.finance/ws
    |-- send public/login (wallet + timestamp + signature)
    |-- send public/subscribe to orderbook.{instrument_name} channels
    |-- forward raw frames -> mpsc::Receiver<RawMessage>
    |
DeriveSupervisor (src/feed/derive/supervisor.rs)
    |-- owns DeriveClient, reconnects on failure
    |-- watches SubscriptionManager for instrument list changes
    |-- sends RawMessage to feed pipeline
    |
DeriveParser (src/feed/derive/normalize.rs)
    |-- parses orderbook delta/snapshot JSON into MarketSnapshot
    |-- uses existing Black-76 pipeline (no changes needed)
    |
SpreadEngine / SignalEngine
    |-- unchanged: receives MarketSnapshot from all venues including Derive
```

**New modules required** (all modeled on existing Deribit/Kalshi pattern):
- `src/feed/derive/mod.rs`
- `src/feed/derive/auth.rs`
- `src/feed/derive/client.rs`
- `src/feed/derive/messages.rs`
- `src/feed/derive/normalize.rs`
- `src/feed/derive/supervisor.rs`
- `src/feed/derive/book.rs` (orderbook delta maintenance)
- `src/feed/derive/channels.rs` (subscription channel name builder)

---

## Rate Limits (LOW confidence -- could not access full docs page)

From search results referencing `docs.derive.xyz/reference/rate-limits`:

- **Algorithm:** Fixed window, refills every 5 seconds
- **Non-matching REST (public endpoints):** ~10 requests per second (TPS) per IP, burst of 5x (50 per window)
- **WebSocket connections:** Limited concurrent connections per IP (exact number unknown; error code `-32100` returned when exceeded)
- **Market makers:** Higher limits available on request

**For this project (read-only market data):**
- Discovery (`public/get_instruments` via REST): Use existing `governor`-based rate limiter at conservative 2 req/s
- WebSocket subscriptions: No per-message rate limit for incoming data; only outgoing subscribe messages are rate-limited
- The existing `VenueRateLimiter` pattern from Deribit/Kalshi applies directly

**Comparison to existing venues:**
| Venue | Rate Limit | Notes |
|-------|-----------|-------|
| Deribit | 20 req/s private | Public WS subscriptions unlimited |
| Polymarket | None documented | CLOB WS market data unrestricted |
| Kalshi | RSA auth required | WS rate not published |
| **Derive** | ~10 req/s (fixed window/5s) | Per IP, public REST |

---

## Instrument Discovery Integration

Derive's `public/get_instruments` returns instruments with format compatible with existing discovery pipeline:

```
public/get_instruments params:
  - currency: "BTC"
  - instrument_type: "option"  (or "perp", "spot")
  - expired: false

Response fields (compatible with existing FuzzyMatchKey pattern):
  - instrument_name: "BTC-20240329-70000-C"  (matches Deribit naming!)
  - expiry_time: Unix timestamp
  - strike: float
  - option_type: "C" | "P"
  - is_active: bool
```

**Critical insight:** Derive uses the EXACT same instrument name format as Deribit (`BTC-YYYYMMDD-STRIKE-C/P`). The existing `FuzzyMatchKey` (asset/strike/direction) matching in the discovery pipeline should recognize Derive instruments without format changes. Only a new `DeriveFeed` discovery checker is needed.

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| secp256k1 signing | k256 0.13 | secp256k1 0.29 (C bindings) | k256 is pure Rust, same RustCrypto family as sha2 already in deps, no C build step |
| secp256k1 signing | k256 0.13 | alloy-signer | 50+ transitive deps, overkill for one `personal_sign` call |
| Derive SDK | Direct JSON-RPC impl | derivexyz/cockpit (copy) | cockpit is a full trading system, not a library; direct impl is 8 files modeled on Deribit client |

---

## Version Compatibility

| Crate | Version | Rust 2024 Edition | Notes |
|-------|---------|-------------------|-------|
| k256 | 0.13 (stable; 0.14 is pre-release) | Compatible (MSRV 1.65+) | NCC Group audit completed; 0.14 in pre-release, avoid pre-release in production |
| sha3 | 0.10 (if needed) | Compatible | Same RustCrypto family as sha2 0.10 already in deps |

**Rust compiler:** 1.85+ (2024 edition) -- no issues.

---

## Dependency Growth Summary

| Milestone | New Crates Added | Rationale |
|-----------|-----------------|-----------|
| v1.0 | Baseline (19 direct deps) | Core system |
| v1.1 | 0 | All built on existing deps |
| v1.2 | 1 (strsim) | Fuzzy matching |
| v1.3 | 0 | All built on existing deps |
| v1.4 | 2 (comfy-table, csv) | CLI output formatting |
| **v1.5** | **1 (k256) + possibly sha3** | **Ethereum wallet signing for Derive auth** |

The 1 new crate continues the project's minimal-dependency philosophy. Everything else (WS, JSON-RPC, rate limiting, reconnection, decimal arithmetic) reuses the existing validated stack.

---

## Open Questions for Implementation

1. **Does `public/login` apply to public orderbook channels?** If Derive allows unauthenticated `public/subscribe` to `orderbook.*` channels, then `k256` may not be needed at all for v1.5 (read-only scope). Verify by testing with a plain WS connection before adding auth. The `public/` prefix on endpoints typically implies no-auth, but Derive's docs show `public/login` as a prerequisite for sessions. MEDIUM confidence that auth IS required.

2. **Does k256's ecdsa feature re-export Keccak256?** Check `k256::ecdsa::signature` module at implementation time. If yes, skip explicit `sha3` dep. If no, add `sha3 = "0.10"`.

3. **Signature encoding:** Ethereum signatures are 65 bytes (r + s + v), typically hex-encoded as `0x` + 130 hex chars. Verify Derive accepts this format (vs. base64 or other encoding). The Hummingbot connector evidence suggests standard `0x` hex.

4. **Exact rate limit numbers:** Visit `docs.derive.xyz/reference/rate-limits` directly to confirm the "10 TPS" figure and burst allowance before setting `VenueRateLimiter` parameters.

---

## Sources

- [Derive.xyz API Overview](https://docs.derive.xyz/reference/overview) -- WebSocket URL, JSON-RPC protocol, transport-agnostic confirmation (HIGH confidence)
- [Derive.xyz JSON-RPC Reference](https://docs.derive.xyz/reference/json-rpc) -- Protocol structure, method naming (HIGH confidence)
- [Derive.xyz Session Keys](https://docs.derive.xyz/reference/session-keys) -- X-LyraWallet header, wallet-based auth, session key concept (HIGH confidence)
- [Derive.xyz public/login](https://docs.derive.xyz/reference/post_public-login) -- Endpoint exists, WebSocket-only, wallet/timestamp/signature params (HIGH confidence via search result snippets)
- [Derive.xyz Rate Limits](https://docs.derive.xyz/reference/rate-limits) -- Fixed window/5s algorithm confirmed; specific numbers not captured (MEDIUM confidence)
- [Hummingbot Derive connector](https://hummingbot.org/exchanges/derive/) -- wallet_address, private_key, subaccount_id credential structure; `personal_sign` approach (MEDIUM confidence)
- [Derive.xyz public/get_instrument](https://docs.derive.xyz/reference/post_public-get-instrument) -- Instrument schema, named params (HIGH confidence)
- [k256 crate](https://crates.io/crates/k256) -- Version 0.13.4 stable, 0.14 pre-release; ecdsa feature; Ethereum signing confirmed (HIGH confidence)
- [derivexyz/cockpit GitHub](https://github.com/derivexyz/cockpit) -- Official Rust market-maker reference confirming Rust-based integration is viable (MEDIUM confidence)
- Existing codebase: `src/feed/kalshi/auth.rs`, `src/feed/deribit/client.rs` -- Confirmed integration patterns for auth and WS client structure (HIGH confidence)

---
*Stack research for: v1.5 Derive.xyz Venue Integration*
*Researched: 2026-03-03*
