//! Release assets on local disk, so devices are served without waiting on GitHub.

use std::{collections::HashSet, path::PathBuf};

use anyhow::Context;
use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use tokio::fs;
use tokio_util::io::ReaderStream;

use super::{AssetRow, parse_version, store, version::pick_latest};
use crate::{
    error::{Result, status},
    state::AppState,
};

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
    if asset.digest.as_ref().is_some_and(|d| *d != sha256) {
        let _ = fs::remove_file(path(state, asset.github_id)).await;
        anyhow::bail!("{} from {repo} does not match GitHub's digest", asset.name);
    }
    store::set_hashes(&state.db, asset.id, &sha256, &md5).await?;
    asset.sha256 = Some(sha256);
    asset.md5 = Some(md5);
    Ok(asset)
}

/// `ensure` for a handler: a failure is logged and the device gets a 502.
pub async fn fetch(state: &AppState, repo: &str, asset: AssetRow) -> Result<AssetRow> {
    ensure(state, repo, asset).await.map_err(|e| {
        tracing::error!("{e:#}");
        status(
            StatusCode::BAD_GATEWAY,
            "firmware not cached and GitHub unavailable",
        )
    })
}

/// The asset's file, fetched from GitHub first if it is not cached; for devices and the admin's
/// first flash.
pub async fn serve(state: &AppState, repo: &str, asset: AssetRow) -> Result<Response> {
    let mut asset = fetch(state, repo, asset).await?;
    let file = match fs::File::open(path(state, asset.github_id)).await {
        // Evicted (`remove_unused`) between `ensure` and here: fetch it again, once.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            asset = fetch(state, repo, asset).await?;
            fs::File::open(path(state, asset.github_id)).await?
        }
        file => file?,
    };
    let headers = [
        (header::CONTENT_TYPE, "application/octet-stream".to_string()),
        (header::CONTENT_LENGTH, asset.size.to_string()),
        (
            header::ETAG,
            format!("\"{}\"", asset.sha256.unwrap_or_default()),
        ),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", asset.name),
        ),
        // ESP32's HTTPUpdate checks the image against this header before booting it.
        (
            header::HeaderName::from_static("x-md5"),
            asset.md5.unwrap_or_default(),
        ),
    ];
    Ok((headers, Body::from_stream(ReaderStream::new(file))).into_response())
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

/// Deletes the cached files of the project's releases that no device runs and that are not the
/// newest (stable or beta): nobody is about to download them. A device that still asks for one
/// gets it fetched from GitHub again, like any uncached asset.
pub async fn remove_unused(state: &AppState, project_id: i64) -> anyhow::Result<()> {
    let releases = store::releases(&state.db, project_id).await?;
    let newest: HashSet<i64> = [false, true]
        .into_iter()
        .filter_map(|beta| pick_latest(releases.clone(), beta).map(|(_, r)| r.id))
        .collect();
    let reported: Vec<(Option<String>,)> =
        sqlx::query_as("SELECT last_version FROM devices WHERE project_id = ?")
            .bind(project_id)
            .fetch_all(&state.db)
            .await?;
    let running: Vec<_> = reported
        .iter()
        .filter_map(|(v,)| v.as_deref().and_then(parse_version))
        .collect();
    // Not while ensure() is writing a file and its hashes.
    let _guard = state.fetch_lock.lock().await;
    for release in releases {
        let in_use = parse_version(&release.tag).is_some_and(|v| running.contains(&v));
        if newest.contains(&release.id) || in_use {
            continue;
        }
        for asset in store::release_assets(&state.db, release.id).await? {
            if asset.sha256.is_none() {
                continue;
            }
            tracing::info!(
                tag = release.tag,
                asset = asset.name,
                "evicting unused asset"
            );
            store::clear_hashes(&state.db, asset.id).await?;
            // A download streaming this file keeps its open handle.
            match fs::remove_file(path(state, asset.github_id)).await {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::tests::setup;

    #[tokio::test]
    async fn evicts_releases_nobody_runs() {
        let (state, dir) = setup().await;
        for sql in [
            "INSERT INTO releases (id, project_id, tag, prerelease, notes, published_at) VALUES (3, 1, 'v1.1.0', 0, 'n', '')",
            "INSERT INTO assets (release_id, name, github_id, size, sha256, md5) VALUES (3, 'firmware.bin', 66, 5, 'cc', 'dd')",
        ] {
            sqlx::query(sql).execute(&state.db).await.unwrap();
        }
        std::fs::write(super::path(&state, 66), b"older").unwrap();
        let cached = |id: i64| super::path(&state, id).exists();

        // A device on v1.1.0 keeps it.
        sqlx::query("UPDATE devices SET last_version = '1.1.0'")
            .execute(&state.db)
            .await
            .unwrap();
        super::remove_unused(&state, 1).await.unwrap();
        assert!(cached(66) && cached(77));

        // Once it moves on, only the newest (v1.2.0) stays.
        sqlx::query("UPDATE devices SET last_version = 'v1.2.0'")
            .execute(&state.db)
            .await
            .unwrap();
        super::remove_unused(&state, 1).await.unwrap();
        assert!(!cached(66) && cached(77));
        let (sha,): (Option<String>,) =
            sqlx::query_as("SELECT sha256 FROM assets WHERE github_id = 66")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(sha, None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
