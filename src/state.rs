use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::{admin::Clerk, github::GitHub};

pub type AppState = Arc<State>;

/// Everything handlers share; built once in `main` (or by the tests).
pub struct State {
    pub db: SqlitePool,
    pub http: reqwest::Client,
    pub github: GitHub,
    pub clerk: Clerk,
    pub cache_dir: PathBuf,
    /// The admin UI's static build (`web/build`).
    pub ui_dir: PathBuf,
    /// One sync at a time, whether from the timer or "Sync now".
    pub sync_lock: Mutex<()>,
    /// One GitHub download at a time (see `firmware::cache::ensure`).
    pub fetch_lock: Mutex<()>,
    /// Device signatures accepted within the clock window, with their timestamps (see `device::auth`).
    pub seen_signatures: std::sync::Mutex<HashMap<Vec<u8>, i64>>,
}

impl State {
    pub fn new(
        db: SqlitePool,
        http: reqwest::Client,
        github: GitHub,
        clerk: Clerk,
        cache_dir: PathBuf,
        ui_dir: PathBuf,
    ) -> AppState {
        Arc::new(Self {
            db,
            http,
            github,
            clerk,
            cache_dir,
            ui_dir,
            sync_lock: Mutex::new(()),
            fetch_lock: Mutex::new(()),
            seen_signatures: Default::default(),
        })
    }
}

/// Unix seconds, the format of every timestamp in the database and the device signature.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
