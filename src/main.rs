mod auth;
mod config;
mod handlers;
mod state;
mod templates;

use std::time::Instant;

#[tokio::main]
async fn main() {
    let config = config::load();

    let state = state::new_app_state(config);
    let bind = state.config.bind.clone();

    tokio::spawn({
        let state = state.clone();
        async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                state.pastes.write().await.retain(|_, entry| Instant::now() < entry.expires_at);
            }
        }
    });

    let app = handlers::build_app(state);

    let listener = tokio::net::TcpListener::bind(&bind).await.unwrap();
    println!("listening on {}", bind);
    axum::serve(listener, app).await.unwrap();
}
