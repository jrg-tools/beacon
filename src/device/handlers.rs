use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::auth::Device;
use crate::{
    error::{Result, status},
    firmware::{cache, store, version},
    state::{AppState, now},
};

/// What a device request did, for the counters and the project page's charts.
enum Seen<'a> {
    Check {
        version: Option<&'a str>,
        channel: &'a str,
    },
    Download {
        bytes: i64,
        tag: &'a str,
    },
}

async fn seen(state: &AppState, device: &Device, what: Seen<'_>) -> Result<()> {
    let (check, version, channel, bytes, tag) = match what {
        Seen::Check { version, channel } => (true, version, Some(channel), None, None),
        Seen::Download { bytes, tag } => (false, None, None, Some(bytes), Some(tag)),
    };
    let at = now();
    // Both or neither: the counters and the charts must agree.
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "UPDATE devices SET last_seen = ?, last_version = COALESCE(?, last_version),
           checks = checks + ?, downloads = downloads + ? WHERE id = ?",
    )
    .bind(at)
    .bind(version)
    .bind(check as i64)
    .bind(!check as i64)
    .bind(&device.id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO device_events (project_id, device_id, at, kind, version, channel, bytes, tag)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(device.project_id)
    .bind(&device.id)
    .bind(at)
    .bind(if check { "check" } else { "download" })
    .bind(version)
    .bind(channel)
    .bind(bytes)
    .bind(tag)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct LatestQuery {
    /// The version the device runs, e.g. `v1.2.3`.
    current: Option<String>,
    /// `stable` (default) or `beta`, which also offers prereleases.
    channel: Option<String>,
}

pub async fn latest(
    State(state): State<AppState>,
    Extension(device): Extension<Device>,
    Path(project): Path<String>,
    Query(q): Query<LatestQuery>,
) -> Result<Json<Value>> {
    device.check_project(&project)?;
    let beta = q.channel.as_deref() == Some("beta");
    let channel = if beta { "beta" } else { "stable" };
    let version = q.current.as_deref();
    // Stored with every check: a version is short, anything longer is not one.
    if version.is_some_and(|v| v.len() > 64) {
        return Err(status(StatusCode::BAD_REQUEST, "current is too long"));
    }
    seen(&state, &device, Seen::Check { version, channel }).await?;
    let (version, release) = store::latest_release(&state.db, device.project_id, beta)
        .await?
        .ok_or(status(StatusCode::NOT_FOUND, "no release"))?;
    let mut assets = Vec::new();
    for mut asset in store::release_assets(&state.db, release.id).await? {
        // Bootloader and partition table: written once by the first flash, never as an update.
        if store::first_flash_names(&device.first_flash).any(|n| n == asset.name) {
            continue;
        }
        // GitHub's digest is the file's SHA-256, so only an asset without one waits on a download.
        // `md5` stays null until the file is cached; the download's `x-md5` header always has it.
        asset.sha256 = asset.sha256.or(asset.digest.clone());
        if asset.sha256.is_none() {
            asset = cache::fetch(&state, &device.repo, asset).await?;
        }
        let mut value = serde_json::to_value(&asset)?;
        value["url"] = format!(
            "/api/v1/firmware/{project}/releases/{}/{}",
            release.tag, asset.name
        )
        .into();
        assets.push(value);
    }
    Ok(Json(json!({
        "project": project,
        // A public project's new device learns its id here (see `auth::issue`).
        "device": device.id,
        "current": q.current,
        "update_available": version::update_available(&version, q.current.as_deref()),
        "latest": {
            "tag": release.tag,
            "version": version.to_string(),
            "prerelease": version::is_prerelease(&release, &version),
            "published_at": release.published_at,
            "notes": release.notes,
            "assets": assets,
        },
    })))
}

pub async fn download(
    State(state): State<AppState>,
    Extension(device): Extension<Device>,
    Path((project, tag, name)): Path<(String, String, String)>,
) -> Result<Response> {
    device.check_project(&project)?;
    // A private project's bootloader and partition table: only the admin's first flash gets them.
    if !device.public && store::first_flash_names(&device.first_flash).any(|n| n == name) {
        return Err(status(
            StatusCode::UNAUTHORIZED,
            "first-flash files are admin only",
        ));
    }
    let asset = store::find_asset(&state.db, device.project_id, &tag, &name)
        .await?
        .ok_or(status(StatusCode::NOT_FOUND, "no such asset"))?;
    let bytes = asset.size;
    seen(&state, &device, Seen::Download { bytes, tag: &tag }).await?;
    cache::serve(&state, &device.repo, asset).await
}
