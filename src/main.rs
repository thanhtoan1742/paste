mod auth;
mod config;
mod handlers;
mod state;
mod templates;

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

    tokio::spawn({
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

    let listener = tokio::net::TcpListener::bind(&bind).await.unwrap();
    println!("listening on {}", bind);
    axum::serve(listener, app).await.unwrap();
}
