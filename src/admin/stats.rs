//! A project's fleet numbers and charts, from the device counters (all time) and `device_events`
//! (the last 90 days).

use std::collections::{BTreeMap, HashMap};

use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::{
    error::Result,
    firmware::{parse_version, store, version},
    state::{AppState, now},
};

/// How many days of history the project page charts.
pub const DAYS: i64 = 30;

#[derive(Serialize, Clone)]
pub struct UpdateStatus {
    pub tag: String,
    /// When the device downloaded it.
    at: i64,
    /// `pending` until the device checks in again; then `succeeded` if it reports `tag`, else `failed`.
    pub status: &'static str,
}

/// Each device's last download and what its next check said about it. Devices only report their
/// version when they check, so a device that never checks again stays `pending`.
pub async fn updates(
    db: &SqlitePool,
    project_id: i64,
) -> sqlx::Result<HashMap<String, UpdateStatus>> {
    let rows: Vec<(String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT d.device_id, d.tag, d.at,
           (SELECT c.version FROM device_events c
            WHERE c.device_id = d.device_id AND c.kind = 'check' AND c.version IS NOT NULL
              AND c.id > d.id
            ORDER BY c.id LIMIT 1)
         FROM device_events d
         WHERE d.project_id = ? AND d.id = (
           SELECT MAX(id) FROM device_events
           WHERE device_id = d.device_id AND kind = 'download' AND tag IS NOT NULL)",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(device, tag, at, next)| {
            let status = match next.as_deref().and_then(parse_version) {
                None if next.is_none() => "pending",
                Some(v) if Some(&v) == parse_version(&tag).as_ref() => "succeeded",
                _ => "failed",
            };
            (device, UpdateStatus { tag, at, status })
        })
        .collect())
}

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
    let updates = updates(&state.db, id).await?;
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
        "updates": updated(&state.db, id, since).await?,
        "updates_pending": in_state("pending"),
        "updates_failed": in_state("failed"),
        "channels": channels(&state.db, id, since).await?,
        "versions": versions,
        "days": days(&state.db, id, today).await?,
    })))
}

/// The last `DAYS` UTC days, oldest first, as unix day numbers; the page turns them into dates.
async fn days(db: &SqlitePool, id: i64, today: i64) -> sqlx::Result<Vec<Value>> {
    let rows: Vec<(i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT at / 86400 AS day, SUM(kind = 'check'), SUM(kind = 'download'),
           COALESCE(SUM(bytes), 0), COUNT(DISTINCT device_id)
         FROM device_events WHERE project_id = ? AND at >= ? GROUP BY day",
    )
    .bind(id)
    .bind((today - DAYS + 1) * 86400)
    .fetch_all(db)
    .await?;
    Ok((today - DAYS + 1..=today)
        .map(|day| {
            let r = rows.iter().find(|r| r.0 == day).copied();
            let (_, checks, downloads, bytes, active) = r.unwrap_or((day, 0, 0, 0, 0));
            json!({ "day": day, "checks": checks, "downloads": downloads, "bytes": bytes, "active": active })
        })
        .collect())
}

/// Devices per channel since `since`.
async fn channels(db: &SqlitePool, id: i64, since: i64) -> sqlx::Result<BTreeMap<String, i64>> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT channel, COUNT(DISTINCT device_id) FROM device_events
         WHERE project_id = ? AND kind = 'check' AND at >= ? GROUP BY channel",
    )
    .bind(id)
    .bind(since)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Updates since `since`: a device reporting a different version than on its previous check.
async fn updated(db: &SqlitePool, id: i64, since: i64) -> sqlx::Result<i64> {
    let (updated,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM (
           SELECT at, version, LAG(version) OVER (PARTITION BY device_id ORDER BY at, id) AS prev
           FROM device_events WHERE project_id = ? AND kind = 'check' AND version IS NOT NULL
         ) WHERE prev IS NOT NULL AND version != prev AND at >= ?",
    )
    .bind(id)
    .bind(since)
    .fetch_one(db)
    .await?;
    Ok(updated)
}
