//! State persistence for checkpoint-based recovery.
//!
//! Provides types and utilities for periodically checkpointing the paper trade
//! engine state to disk and restoring it on startup. Uses atomic file writes
//! (write-to-temp-then-rename) for crash safety.

pub mod atomic;
pub mod checkpoint;
pub mod recovery;

pub use checkpoint::CheckpointState;
pub use recovery::{load_checkpoint, replay_trade_events};
