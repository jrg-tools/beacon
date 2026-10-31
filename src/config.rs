//! Settings from the environment; `.env.example` documents each one.

use std::{env, path::PathBuf, time::Duration};

use anyhow::Context;

pub struct Config {
    pub data_dir: PathBuf,
    pub ui_dir: PathBuf,
    pub bind: String,
    pub github_token: Option<String>,
    pub clerk_secret_key: String,
    pub clerk_publishable_key: String,
    pub clerk_scope: String,
    pub sync_interval: Duration,
}

fn required(name: &str) -> anyhow::Result<String> {
    env::var(name).with_context(|| format!("{name} is not set"))
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.is_empty())
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let sync_secs = match optional("SYNC_INTERVAL_SECS") {
            Some(s) => s
                .parse()
                .context("SYNC_INTERVAL_SECS must be a number of seconds")?,
            None => 2 * 24 * 60 * 60, // two days
        };
        Ok(Self {
            data_dir: optional("DATA_DIR").unwrap_or_else(|| "data".into()).into(),
            ui_dir: optional("UI_DIR")
                .unwrap_or_else(|| "web/build".into())
                .into(),
            bind: optional("BIND").unwrap_or_else(|| "0.0.0.0:8080".into()),
            github_token: optional("GITHUB_TOKEN"),
            clerk_secret_key: required("CLERK_SECRET_KEY")?,
            clerk_publishable_key: required("CLERK_PUBLISHABLE_KEY")?,
            clerk_scope: optional("CLERK_SCOPE").unwrap_or_else(|| "beacon".into()),
            sync_interval: Duration::from_secs(sync_secs),
        })
    }
}
