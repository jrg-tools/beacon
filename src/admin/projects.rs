use std::collections::BTreeMap;

use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value, json};
use tokio_util::io::ReaderStream;

use super::project_input::ProjectInput;
use crate::{
    error::{AppError, Result, status},
    firmware::{cache, parse_version, store, sync, version},
    state::{AppState, now},
};

fn conflict_or(e: sqlx::Error) -> AppError {
    if e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
    {
        status(StatusCode::CONFLICT, "slug already used")
    } else {
        e.into()
    }
}

#[derive(sqlx::FromRow, Serialize)]
pub struct ProjectRow {
    id: i64,
    slug: String,
    name: String,
    emoji: String,
    repo: String,
    assets: String,
    synced_at: Option<i64>,
    sync_error: Option<String>,
    releases: i64,
    devices: i64,
    #[sqlx(skip)]
    latest: Option<String>,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<ProjectRow>>> {
    let mut rows: Vec<ProjectRow> = sqlx::query_as(
        "SELECT p.id, p.slug, p.name, p.emoji, p.repo, p.assets, p.synced_at, p.sync_error,
           (SELECT COUNT(*) FROM releases WHERE project_id = p.id) AS releases,
           (SELECT COUNT(*) FROM devices WHERE project_id = p.id) AS devices
         FROM projects p ORDER BY p.name",
    )
    .fetch_all(&state.db)
    .await?;
    for row in &mut rows {
        row.latest = store::latest_release(&state.db, row.id, false)
            .await?
            .map(|(_, r)| r.tag);
    }
    Ok(Json(rows))
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<ProjectInput>,
) -> Result<Json<Value>> {
    let p = input.validate()?;
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO projects (slug, name, emoji, repo, assets, created_at) VALUES (?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(&p.slug)
    .bind(&p.name)
    .bind(&p.emoji)
    .bind(&p.repo)
    .bind(&p.assets)
    .bind(now())
    .fetch_one(&state.db)
    .await
    .map_err(conflict_or)?;
    sync::spawn(state, id);
    Ok(Json(json!({ "id": id })))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<ProjectInput>,
) -> Result<StatusCode> {
    let p = input.validate()?;
    let done = sqlx::query(
        "UPDATE projects SET slug = ?, name = ?, emoji = ?, repo = ?, assets = ? WHERE id = ?",
    )
    .bind(&p.slug)
    .bind(&p.name)
    .bind(&p.emoji)
    .bind(&p.repo)
    .bind(&p.assets)
    .bind(id)
    .execute(&state.db)
    .await
    .map_err(conflict_or)?;
    if done.rows_affected() == 0 {
        return Err(status(StatusCode::NOT_FOUND, "no such project"));
    }
    sync::spawn(state, id);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<i64>) -> Result<StatusCode> {
    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    cache::remove_orphans(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn sync(State(state): State<AppState>, Path(id): Path<i64>) -> StatusCode {
    sync::spawn(state, id);
    StatusCode::ACCEPTED
}

pub async fn releases(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<Value>>> {
    let devices: Vec<(Option<String>, String)> =
        sqlx::query_as("SELECT last_version, id FROM devices WHERE project_id = ?")
            .bind(id)
            .fetch_all(&state.db)
            .await?;
    // Compared as semver, so a device reporting `1.2.0` counts for the tag `v1.2.0`.
    let installed: Vec<_> = devices
        .iter()
        .filter_map(|(v, _)| v.as_deref().and_then(parse_version))
        .collect();
    // Only current devices: a revoked one would stay pending forever.
    let mut updates = super::devices::updates(&state.db, id).await?;
    updates.retain(|device, _| devices.iter().any(|d| &d.1 == device));
    let mut out = Vec::new();
    for r in store::releases(&state.db, id).await? {
        let assets = store::release_assets(&state.db, r.id).await?;
        let version = parse_version(&r.tag);
        let devices = installed
            .iter()
            .filter(|v| Some(*v) == version.as_ref())
            .count();
        let pending = updates
            .values()
            .filter(|u| u.status == "pending" && u.tag == r.tag)
            .count();
        out.push(json!({ "tag": r.tag, "prerelease": r.prerelease, "published_at": r.published_at, "devices": devices, "pending": pending, "assets": assets }));
    }
    Ok(Json(out))
}

/// How many days of history the project page charts.
pub const DAYS: i64 = 30;

/// A device's last version, last seen, checks, downloads and id.
type DeviceTotals = (Option<String>, Option<i64>, i64, i64, String);

/// Fleet numbers and daily series for one project's page.
pub async fn stats(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Json<Value>> {
    let devices: Vec<DeviceTotals> = sqlx::query_as(
        "SELECT last_version, last_seen, checks, downloads, id FROM devices WHERE project_id = ?",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    let latest = store::latest_release(&state.db, id, false).await?;
    let t = now();
    let today = t / 86400;
    let since = (today - DAYS + 1) * 86400;
    let seen_since = |secs: i64| {
        devices
            .iter()
            .filter(|d| d.1.is_some_and(|at| at > t - secs))
            .count()
    };
    let mut versions = BTreeMap::<&str, i64>::new();
    for (v, ..) in &devices {
        *versions
            .entry(v.as_deref().unwrap_or("unknown"))
            .or_default() += 1;
    }

    // Days are UTC, as unix day numbers; the page turns them into dates.
    let rows: Vec<(i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT at / 86400 AS day, SUM(kind = 'check'), SUM(kind = 'download'),
           COALESCE(SUM(bytes), 0), COUNT(DISTINCT device_id)
         FROM device_events WHERE project_id = ? AND at >= ? GROUP BY day",
    )
    .bind(id)
    .bind(since)
    .fetch_all(&state.db)
    .await?;
    let days: Vec<Value> = (today - DAYS + 1..=today)
        .map(|day| {
            let r = rows.iter().find(|r| r.0 == day).copied();
            let (_, checks, downloads, bytes, active) = r.unwrap_or((day, 0, 0, 0, 0));
            json!({ "day": day, "checks": checks, "downloads": downloads, "bytes": bytes, "active": active })
        })
        .collect();
    let channels: Vec<(String, i64)> = sqlx::query_as(
        "SELECT channel, COUNT(DISTINCT device_id) FROM device_events
         WHERE project_id = ? AND kind = 'check' AND at >= ? GROUP BY channel",
    )
    .bind(id)
    .bind(since)
    .fetch_all(&state.db)
    .await?;
    // An update is a device reporting a different version than on its previous check.
    let (updated,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM (
           SELECT at, version, LAG(version) OVER (PARTITION BY device_id ORDER BY at, id) AS prev
           FROM device_events WHERE project_id = ? AND kind = 'check' AND version IS NOT NULL
         ) WHERE prev IS NOT NULL AND version != prev AND at >= ?",
    )
    .bind(id)
    .bind(since)
    .fetch_one(&state.db)
    .await?;

    let updates = super::devices::updates(&state.db, id).await?;
    let in_state = |status: &str| {
        devices
            .iter()
            .filter(|d| updates.get(&d.4).is_some_and(|u| u.status == status))
            .count()
    };

    Ok(Json(json!({
        "devices": devices.len(),
        "active_24h": seen_since(86400),
        "active_7d": seen_since(7 * 86400),
        "up_to_date": latest.as_ref().map_or(0, |(l, _)| {
            devices.iter().filter(|d| !version::update_available(l, d.0.as_deref())).count()
        }),
        "latest": latest.map(|(_, r)| r.tag),
        "checks": devices.iter().map(|d| d.2).sum::<i64>(),
        "downloads": devices.iter().map(|d| d.3).sum::<i64>(),
        "updates": updated,
        "updates_pending": in_state("pending"),
        "updates_failed": in_state("failed"),
        "channels": channels.into_iter().collect::<BTreeMap<_, _>>(),
        "versions": versions,
        "days": days,
    })))
}

/// A release asset for the firmware page to flash over USB; not counted as a device download.
pub async fn asset(
    State(state): State<AppState>,
    Path((id, tag, name)): Path<(i64, String, String)>,
) -> Result<Response> {
    let asset = store::find_asset(&state.db, id, &tag, &name)
        .await?
        .ok_or(status(StatusCode::NOT_FOUND, "no such asset"))?;
    let (repo,): (String,) = sqlx::query_as("SELECT repo FROM projects WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let asset = cache::ensure(&state, &repo, asset).await.map_err(|e| {
        tracing::error!("{e:#}");
        status(
            StatusCode::BAD_GATEWAY,
            "firmware not cached and GitHub unavailable",
        )
    })?;
    let file = tokio::fs::File::open(cache::path(&state, asset.github_id)).await?;
    let headers = [
        (header::CONTENT_TYPE, "application/octet-stream".to_string()),
        (header::CONTENT_LENGTH, asset.size.to_string()),
    ];
    Ok((headers, Body::from_stream(ReaderStream::new(file))).into_response())
}
