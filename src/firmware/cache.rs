//! Release assets on local disk, so devices are served without waiting on GitHub.

use std::{collections::HashSet, path::PathBuf};

use anyhow::Context;
use tokio::fs;

use super::{AssetRow, store};
use crate::state::AppState;

pub fn path(state: &AppState, github_id: i64) -> PathBuf {
    state.cache_dir.join(format!("{github_id}.bin"))
}

async fn is_cached(state: &AppState, asset: &AssetRow) -> anyhow::Result<bool> {
    Ok(asset.sha256.is_some() && fs::try_exists(path(state, asset.github_id)).await?)
}

/// Downloads the asset from GitHub unless it is already cached; returns it with its hashes.
pub async fn ensure(state: &AppState, repo: &str, asset: AssetRow) -> anyhow::Result<AssetRow> {
    if is_cached(state, &asset).await? {
        return Ok(asset);
    }
    // shortcut: one download at a time across all assets; fine for a handful of projects.
    let _guard = state.fetch_lock.lock().await;
    let mut asset = store::asset(&state.db, asset.id).await?;
    if is_cached(state, &asset).await? {
        return Ok(asset); // another request fetched it while we waited
    }
    tracing::info!(repo, asset = asset.name, "caching asset");
    let (sha256, md5) = state
        .github
        .download(
            repo,
            asset.github_id,
            asset.size,
            &path(state, asset.github_id),
        )
        .await
        .with_context(|| format!("downloading {} from {repo}", asset.name))?;
    store::set_hashes(&state.db, asset.id, &sha256, &md5).await?;
    asset.sha256 = Some(sha256);
    asset.md5 = Some(md5);
    Ok(asset)
}

/// Deletes cached files no asset row points at any more.
pub async fn remove_orphans(state: &AppState) -> anyhow::Result<()> {
    let ids: Vec<(i64,)> = sqlx::query_as("SELECT github_id FROM assets")
        .fetch_all(&state.db)
        .await?;
    let live: HashSet<String> = ids.into_iter().map(|(id,)| format!("{id}.bin")).collect();
    let mut dir = fs::read_dir(&state.cache_dir).await?;
    while let Some(entry) = dir.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        // `.part` files belong to a download that may be running right now.
        if name.ends_with(".bin") && !live.contains(&name) {
            fs::remove_file(entry.path()).await?;
        }
    }
    Ok(())
}
