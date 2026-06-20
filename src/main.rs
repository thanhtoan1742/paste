mod auth;
mod config;
mod handlers;
mod state;
mod templates;

use std::future::IntoFuture;
use std::time::Instant;

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

    println!("shutdown signal received, draining connections...");
    let _ = shutdown_tx.send(());
}

#[tokio::main]
async fn main() {
    let path = parse_config_path();
    let config = match config::load(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let state = state::new_app_state(config);
    let bind = state.config.bind.clone();

    let sweeper = tokio::spawn({
        let state = state.clone();
        async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                handlers::SWEEPER_INTERVAL_SECS,
            ));
            loop {
                interval.tick().await;
                state
                    .pastes
                    .write()
                    .await
                    .retain(|_, entry| Instant::now() < entry.expires_at);
            }
        }
    });

    let app = handlers::build_app(state);

    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error binding to {bind}: {e}");
            sweeper.abort();
            std::process::exit(1);
        }
    };
    println!("listening on {}", bind);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown_tx))
        .into_future();
    tokio::pin!(server);

    // Phase 1: run until a signal fires (or the server ends on its own).
    tokio::select! {
        res = &mut server => {
            match res {
                Ok(()) => eprintln!("server exited before any signal"),
                Err(e) => eprintln!("server error: {e}"),
            }
            sweeper.abort();
            std::process::exit(1);
        }
        _ = shutdown_rx => {
            // Signal received; axum has stopped accepting new connections.
        }
    }

    sweeper.abort();

    // Phase 2: cap the drain at 10 seconds.
    match tokio::time::timeout(std::time::Duration::from_secs(10), &mut server).await {
        Ok(Ok(())) => println!("shutdown complete"),
        Ok(Err(e)) => {
            eprintln!("server error during shutdown: {e}");
            std::process::exit(1);
        }
        Err(_) => {
            println!("shutdown timed out after 10s, forcing exit");
            std::process::exit(1);
        }
    }
}
