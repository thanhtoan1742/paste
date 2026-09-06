mod auth;
mod base64;
mod config;
mod handlers;
mod multipart;
mod state;
mod templates;

use std::future::IntoFuture;
use std::time::Instant;
use tracing::{error, info, warn};

fn init_logging() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::SystemTime)
        .init();
}

fn parse_config_path() -> String {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if (args[i] == "--config" || args[i] == "-c") && i + 1 < args.len() {
            return args[i + 1].clone();
        }
        i += 1;
    }
    "paste.toml".to_string()
}

async fn shutdown_signal(shutdown_tx: tokio::sync::oneshot::Sender<()>) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }

    warn!("shutdown signal received, draining connections...");
    let _ = shutdown_tx.send(());
}

#[tokio::main]
async fn main() {
    init_logging();

    let path = parse_config_path();
    info!(config_path = %path, "loading config");
    let config = match config::load(&path) {
        Ok(c) => c,
        Err(e) => {
            error!(config_path = %path, error = %e, "failed to load config");
            std::process::exit(1);
        }
    };

    let state = state::new_app_state(config);
    let bind = state.config.bind.clone();
    info!(
        bind = %bind,
        max_pastes = state.config.max_pastes,
        max_size = state.config.max_size,
        max_ttl_secs = state.config.max_ttl_secs,
        lockdown = state.config.lockdown,
        prefix = %state.config.prefix,
        "starting paste server"
    );

    let sweeper = tokio::spawn({
        let state = state.clone();
        async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                handlers::SWEEPER_INTERVAL_SECS,
            ));
            loop {
                interval.tick().await;
                let mut pastes = state.pastes.write().await;
                let before = pastes.len();
                pastes.retain(|_, entry| Instant::now() < entry.expires_at);
                let removed = before - pastes.len();
                if removed > 0 {
                    info!(removed = removed, remaining = pastes.len(), "sweeper removed expired pastes");
                }
            }
        }
    });

    let app = handlers::build_app(state);

    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            error!(bind = %bind, error = %e, "failed to bind listener");
            sweeper.abort();
            std::process::exit(1);
        }
    };
    info!(bind = %bind, "listening");

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown_tx))
        .into_future();
    tokio::pin!(server);

    // Phase 1: run until a signal fires (or the server ends on its own).
    tokio::select! {
        biased;
        _ = shutdown_rx => {
            // Signal received; axum has stopped accepting new connections.
        }
        res = &mut server => {
            match res {
                Ok(()) => error!("server exited before any signal"),
                Err(e) => error!(error = %e, "server error"),
            }
            sweeper.abort();
            std::process::exit(1);
        }
    }

    sweeper.abort();

    // Phase 2: cap the drain at 10 seconds.
    match tokio::time::timeout(std::time::Duration::from_secs(10), &mut server).await {
        Ok(Ok(())) => info!("shutdown complete"),
        Ok(Err(e)) => {
            error!(error = %e, "server error during shutdown");
            std::process::exit(1);
        }
        Err(_) => {
            warn!("shutdown timed out after 10s, forcing exit");
            std::process::exit(1);
        }
    }
}
