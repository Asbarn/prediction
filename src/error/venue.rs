// Placeholder -- implemented in Task 2.
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorSeverity {
    Fatal,
    Degraded,
    Transient,
}

#[derive(Debug, thiserror::Error)]
pub enum VenueError {
    #[error("placeholder")]
    _Placeholder,
}
