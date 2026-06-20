# Graceful SIGTERM/SIGINT Handling

## Problem

`src/main.rs` currently calls `axum::serve(listener, app).await.unwrap()` with no signal handling. On SIGTERM or SIGINT the process is killed immediately, aborting in-flight requests. The background sweeper task also runs an unbounded `loop` and its `JoinHandle` is discarded. This is hostile to container orchestrators (Docker/Kubernetes send SIGTERM with a limited grace period) and to interactive Ctrl-C use during active requests.

## Goal

On SIGTERM or SIGINT:
1. Stop accepting new connections.
2. Let in-flight requests drain, with a hard cap of 10 seconds.
3. Cleanly cancel the sweeper background task.
4. Exit with a clear log line and a sensible status code in all cases.

## Non-Goals

- No persistent state to flush (pastes are in-memory only).
- No new runtime crates (the existing `tokio` dependency gains the `"signal"` feature — see below).
- No config option for the timeout (fixed at 10s per the approved design).
- No changes to the existing unit tests, which use one-shot `ServiceExt` calls rather than a running server.

## Approach

Change is localized to `src/main.rs` plus a one-line edit to `Cargo.toml`. Uses axum 0.8's built-in `with_graceful_shutdown` and standard `tokio` primitives. No new crates.

### Dependency change

`tokio::signal` (both `ctrl_c()` and `unix::signal(...)`) is gated behind the `"signal"` feature, which the current `Cargo.toml` does not enable. Add it to the existing tokio dependency:

```toml
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "time", "sync", "signal"] }
```

This enables the `signal` module without adding a new crate.

### Components

#### 1. `shutdown_signal(shutdown_tx: oneshot::Sender<()>)`

An async function passed to `axum::serve(...).with_graceful_shutdown(...)`. It:

- Races two signal sources with `tokio::select!`:
  - SIGINT via `tokio::signal::ctrl_c()`.
  - SIGTERM via `tokio::signal::unix::signal(SignalKind::terminate())`, compiled only under `#[cfg(unix)]`. On non-Unix targets this branch is `std::future::pending::<()>` so the select reduces to "wait for ctrl-c".
- On either signal, logs `shutdown signal received, draining connections...` and sends `()` through the oneshot channel.
- Signal-handler installation failures are unrecoverable, so they use `.expect(...)` with a descriptive message (mirroring the convention used by `tokio` examples).

The oneshot send is what tells `main` that the signal has fired, independently of axum's internal drain state.

#### 2. Reworked `main()`

```
parse config, build state, bind listener        (existing)
sweeper = tokio::spawn(...)                      (keep the JoinHandle)
(server_tx, server_rx) = oneshot::channel()
server = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal(server_tx))
pin!(server)

// Phase 1: run server until a signal fires
select! {
    res = &mut server => {
        // server ended before any signal (bind/serve error)
        log + exit non-zero
    }
    _ = server_rx => {
        // signal received; axum has stopped accepting new connections
    }
}

// Sweeper is no longer needed
sweeper.abort()

// Phase 2: cap the drain at 10s
match timeout(Duration::from_secs(10), &mut server).await {
    Ok(Ok(()))  => println!("shutdown complete")
    Ok(Err(e))  => eprintln!("server error during shutdown: {e}"); exit(1)
    Err(_)      => println!("shutdown timed out after 10s, forcing exit"); exit(1)
}
```

#### 3. Error handling cleanup

- `TcpListener::bind(&bind).await` — replace `.unwrap()` with a clean error message and non-zero exit, consistent with the existing config-load error path.
- `axum::serve(...).await` — the `.unwrap()` is removed as a direct consequence of the two-phase select; all outcomes are matched and logged.

## Data Flow

```
SIGINT or SIGTERM received
  -> shutdown_signal() resolves
  -> oneshot send fires
  -> axum stops accepting new connections (built-in with_graceful_shutdown behavior)
  -> main Phase 1 select completes via server_rx
  -> sweeper.abort()
  -> Phase 2: race server drain vs 10s timeout
       drain completes    -> "shutdown complete", exit 0
       10s elapses        -> "shutdown timed out after 10s, forcing exit", exit 1
       server errors      -> log error, exit 1
```

## Exit Codes

| Outcome                                  | Exit code |
|------------------------------------------|-----------|
| Graceful drain within 10s                | 0         |
| Drain timeout (forced exit)              | 1         |
| Server error during shutdown drain       | 1         |
| Server error during normal operation     | 1         |
| Bind error at startup                    | 1         |
| Config load error at startup             | 1 (existing) |

## Testing

Signal handling is integration-level and not amenable to the existing unit-test style (which uses one-shot `ServiceExt::oneshot` calls against `build_app`, not a running server). Writing fake-signal unit tests tends to be fragile and not representative.

Verification will be manual:

1. `cargo run` with a valid `paste.toml`.
2. Confirm `listening on <bind>` appears.
3. Issue a request, send `SIGTERM` (or Ctrl-C) mid-flight, confirm the response completes and `shutdown signal received, draining connections...` then `shutdown complete` is logged and the process exits 0.
4. Repeat with a hung/in-flight request; confirm the 10s timeout fires and `shutdown timed out after 10s, forcing exit` is logged with exit code 1.
5. Confirm `cargo build` and `cargo test` (existing suite) still pass.
6. Confirm `cargo clippy` (if configured) stays clean.

No new automated tests will be added for the signal path itself.

## Files Touched

- `Cargo.toml` — add `"signal"` to tokio's `features` list.
- `src/main.rs` — add `shutdown_signal()`, rework `main()`, store and abort the sweeper handle.

No other files change.
