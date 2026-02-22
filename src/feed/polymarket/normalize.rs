//! Polymarket message processor and normalization pipeline.
//!
//! Receives raw WebSocket frames via an mpsc channel, parses them as
//! Polymarket events, and converts book snapshots into `MarketSnapshot`
//! events with probability fields populated.
//!
//! Implemented in Task 2 (04-01-PLAN).
