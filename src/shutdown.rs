use tokio_util::sync::CancellationToken;

/// Listen for OS shutdown signals and cancel the token on receipt.
///
/// On all platforms: listens for Ctrl+C (SIGINT).
/// On Unix: also listens for SIGTERM.
///
/// When a signal is received, logs the signal name at INFO level and
/// cancels the provided `CancellationToken`, triggering graceful shutdown
/// for all holders of the token or its children.
///
/// # Panics
///
/// Panics if the OS cannot install signal handlers -- this is a true
/// invariant (if the OS can't handle signals, we can't run).
pub async fn shutdown_signal(token: CancellationToken) {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        )
        .expect("failed to install SIGTERM handler");

        tokio::select! {
            result = ctrl_c => {
                result.expect("failed to listen for ctrl-c");
                tracing::info!("received SIGINT, initiating shutdown");
            }
            _ = sigterm.recv() => {
                tracing::info!("received SIGTERM, initiating shutdown");
            }
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.expect("failed to listen for ctrl-c");
        tracing::info!("received Ctrl+C, initiating shutdown");
    }

    token.cancel();
}
