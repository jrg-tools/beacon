use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Serialize;
use serde_json::{Value, json};

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
    let installed: Vec<(Option<String>,)> =
        sqlx::query_as("SELECT last_version FROM devices WHERE project_id = ?")
            .bind(id)
            .fetch_all(&state.db)
            .await?;
    // Compared as semver, so a device reporting `1.2.0` counts for the tag `v1.2.0`.
    let installed: Vec<_> = installed
        .iter()
        .filter_map(|(v,)| v.as_deref().and_then(parse_version))
        .collect();
    let mut out = Vec::new();
    for r in store::releases(&state.db, id).await? {
        let assets = store::release_assets(&state.db, r.id).await?;
        let version = parse_version(&r.tag);
        let devices = installed
            .iter()
            .filter(|v| Some(*v) == version.as_ref())
            .count();
        out.push(json!({ "tag": r.tag, "prerelease": r.prerelease, "published_at": r.published_at, "devices": devices, "assets": assets }));
    }
    Ok(Json(out))
}

/// How many days of history the project page charts.
pub const DAYS: i64 = 30;

/// Fleet numbers and daily series for one project's page.
pub async fn stats(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Json<Value>> {
    let devices: Vec<(Option<String>, Option<i64>, i64, i64)> = sqlx::query_as(
        "SELECT last_version, last_seen, checks, downloads FROM devices WHERE project_id = ?",
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
    let (updates,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM (
           SELECT at, version, LAG(version) OVER (PARTITION BY device_id ORDER BY at, id) AS prev
           FROM device_events WHERE project_id = ? AND kind = 'check' AND version IS NOT NULL
         ) WHERE prev IS NOT NULL AND version != prev AND at >= ?",
    )
    .bind(id)
    .bind(since)
    .fetch_one(&state.db)
    .await?;

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
        "updates": updates,
        "channels": channels.into_iter().collect::<BTreeMap<_, _>>(),
        "versions": versions,
        "days": days,
    })))
}
