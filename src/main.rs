mod admin;
mod app;
mod config;
mod db;
mod device;
mod error;
mod firmware;
mod github;
mod install;
mod state;

use std::time::Duration;

use tokio::signal::unix::{SignalKind, signal};

use crate::{config::Config, state::State};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let config = Config::from_env()?;

    let cache_dir = config.data_dir.join("cache");
    tokio::fs::create_dir_all(&cache_dir).await?;
    let db = db::open(&config.data_dir.join("beacon.db")).await?;
    let http = reqwest::Client::builder()
        .user_agent(concat!("beacon/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        // Per read, not the whole body: firmware downloads may take longer than this.
        .read_timeout(Duration::from_secs(30))
        .build()?;
    let github = github::GitHub {
        http: http.clone(),
        token: config.github_token,
    };
    let clerk = admin::Clerk::new(
        config.clerk_secret_key,
        config.clerk_publishable_key,
        config.clerk_scope,
    )?;
    let state = State::new(db, http, github, clerk, cache_dir, config.ui_dir);

    tokio::spawn(firmware::sync::every(state.clone(), config.sync_interval));

    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    tracing::info!("listening on {}", config.bind);
    axum::serve(listener, app::router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let mut term = signal(SignalKind::terminate()).expect("installing the SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[cfg(test)]
mod tests;
