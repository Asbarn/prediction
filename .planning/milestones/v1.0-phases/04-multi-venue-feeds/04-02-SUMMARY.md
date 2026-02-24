---
phase: 04-multi-venue-feeds
plan: 02
subsystem: feed
tags: [kalshi, websocket, rsa-pss, auth, order-book, btreemap, cents-probability]

# Dependency graph
requires:
  - phase: 03-feed-infrastructure
    provides: "DeribitSupervisor pattern, RawMessage/RecordLine traits, exponential backoff"
provides:
  - "Kalshi RSA-PSS authentication (sign_kalshi_request)"
  - "Kalshi WebSocket client with auth headers"
  - "Kalshi message types (orderbook_snapshot, orderbook_delta)"
  - "KalshiBook: incremental BTreeMap order book with derived asks"
  - "KalshiProcessor: cents-to-probability normalization"
  - "KalshiSupervisor: reconnection with fresh auth per attempt"
affects: [04-03, 05-event-mapping]

# Tech tracking
tech-stack:
  added:
    - "rsa 0.9 (RSA-PSS signing)"
    - "sha2 0.10 (SHA-256 for signature digest)"
    - "base64 0.22 (signature encoding)"
  patterns:
    - "BTreeMap ascending sort: best bid via .last() (Pitfall 3)"
    - "Derived asks from complementary side: YES ask = 100 - best NO bid (Pitfall 2)"
    - "Cents-to-probability: Decimal::new(cents, 2) -- 42 -> 0.42"
    - "Fresh auth signature per connection (timestamp in signing message)"

key-files:
  created:
    - src/feed/kalshi/mod.rs
    - src/feed/kalshi/auth.rs
    - src/feed/kalshi/messages.rs
    - src/feed/kalshi/client.rs
    - src/feed/kalshi/book.rs
    - src/feed/kalshi/normalize.rs
    - src/feed/kalshi/supervisor.rs
  modified:
    - Cargo.toml
    - src/config/venues.rs
    - src/config/credentials.rs
    - config/venues.toml
    - tests/smoke_test.rs

key-decisions:
  - "Credentials switched from email/password to kalshi_api_key_id + kalshi_private_key (RSA-PSS)"
  - "KalshiMessage::parse() uses manual JSON inspection rather than serde tagged enum (defensive)"
  - "BTreeMap for order book levels -- ascending sort means .last() for best bid"
  - "YES contract perspective for MarketSnapshot -- bid from YES bids, ask derived from NO bids"
  - "SignatureEncoding trait import required for rsa::pss::Signature::to_bytes()"

patterns-established:
  - "Incremental order book with BTreeMap: apply_snapshot + apply_delta pattern"
  - "Complementary side ask derivation for binary outcome markets"
  - "Manual JSON dispatch for venues with inconsistent message tagging"

# Metrics
duration: ~15min
completed: 2026-02-22
---

# Phase 4 Plan 2: Kalshi RSA-PSS Client Summary

**Kalshi WebSocket client with RSA-PSS auth, incremental BTreeMap order book, cents-to-probability normalization, and reconnection supervisor**

## Performance

- **Duration:** ~15 min
- **Tasks:** 2
- **Files created:** 7
- **Files modified:** 5

## Accomplishments
- RSA-PSS signing module (load_kalshi_private_key, sign_kalshi_request) with SHA-256
- KalshiClient authenticates with KALSHI-ACCESS-KEY/SIGNATURE/TIMESTAMP headers
- Message types handle orderbook_snapshot and orderbook_delta with defensive parsing
- KalshiBook: BTreeMap-based incremental order book with correct ascending-sort handling
- Derived asks: YES ask = 100 - best NO bid (Pitfall 2)
- KalshiProcessor: cents-to-probability conversion (42 cents -> 0.42)
- KalshiSupervisor: fresh RSA-PSS auth signature per reconnection attempt
- 28 unit tests: auth signing, message parsing, book management, normalization

## Task Commits

1. **Task 1: Auth, config, messages, WebSocket client** - `f3c2b27` (combined commit)
2. **Task 2: Book, processor, supervisor** - `76c7ac1` (combined commit)

## Deviations from Plan

- Fixed missing `SignatureEncoding` trait import for `to_bytes()` on RSA signature
- Updated smoke_test.rs to use new credential field names (kalshi_api_key_id, kalshi_private_key)

## Self-Check: PASSED

- All 7 Kalshi files verified on disk
- Commits verified in git log
- All tests pass including RSA key generation tests

---
*Phase: 04-multi-venue-feeds*
*Completed: 2026-02-22*
