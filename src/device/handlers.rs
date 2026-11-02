use axum::{
    Extension, Json,
    body::Body,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::io::ReaderStream;

use super::auth::Device;
use crate::{
    error::{Result, status},
    firmware::{AssetRow, cache, store, version},
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
    let (kind, version, channel, bytes, tag) = match what {
        Seen::Check { version, channel } => ("check", version, Some(channel), None, None),
        Seen::Download { bytes, tag } => ("download", None, None, Some(bytes), Some(tag)),
    };
    let check = kind == "check";
    let at = now();
    sqlx::query(
        "UPDATE devices SET last_seen = ?, last_version = COALESCE(?, last_version),
           checks = checks + ?, downloads = downloads + ? WHERE id = ?",
    )
    .bind(at)
    .bind(version)
    .bind(check as i64)
    .bind(!check as i64)
    .bind(&device.id)
    .execute(&state.db)
    .await?;
    sqlx::query(
        "INSERT INTO device_events (project_id, device_id, at, kind, version, channel, bytes, tag)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(device.project_id)
    .bind(&device.id)
    .bind(at)
    .bind(kind)
    .bind(version)
    .bind(channel)
    .bind(bytes)
    .bind(tag)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn cached(state: &AppState, repo: &str, asset: AssetRow) -> Result<AssetRow> {
    cache::ensure(state, repo, asset).await.map_err(|e| {
        tracing::error!("{e:#}");
        status(
            StatusCode::BAD_GATEWAY,
            "firmware not cached and GitHub unavailable",
        )
    })
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
    seen(&state, &device, Seen::Check { version, channel }).await?;
    let (version, release) = store::latest_release(&state.db, device.project_id, beta)
        .await?
        .ok_or(status(StatusCode::NOT_FOUND, "no release"))?;
    let mut assets = Vec::new();
    for asset in store::release_assets(&state.db, release.id).await? {
        // Bootloader and partition table: written once by the first flash, never as an update.
        if store::first_flash_names(&device.first_flash).any(|n| n == asset.name) {
            continue;
        }
        let asset = cached(&state, &device.repo, asset).await?;
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
    serve(&state, &device.repo, asset).await
}

/// The asset's file, fetched from GitHub first if it is not cached; also the admin's first flash.
pub async fn serve(state: &AppState, repo: &str, asset: AssetRow) -> Result<Response> {
    let asset = cached(state, repo, asset).await?;
    let file = tokio::fs::File::open(cache::path(state, asset.github_id)).await?;
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
