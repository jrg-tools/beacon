use aws_lc_rs::signature::{ED25519, ParsedPublicKey};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use std::collections::HashMap;

use sqlx::SqlitePool;

use super::projects::DAYS;
use crate::{
    error::{Result, status},
    firmware::{parse_version, store, version},
    state::{AppState, now},
};

#[derive(sqlx::FromRow, Serialize)]
pub struct DeviceRow {
    id: String,
    name: String,
    created_at: i64,
    last_seen: Option<i64>,
    last_version: Option<String>,
    checks: i64,
    downloads: i64,
    #[sqlx(skip)]
    up_to_date: bool,
    /// Requests per day over the project page's chart window, oldest first.
    #[sqlx(skip)]
    activity: Vec<i64>,
    #[sqlx(skip)]
    update: Option<UpdateStatus>,
}

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

pub async fn list(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<DeviceRow>>> {
    let mut rows: Vec<DeviceRow> = sqlx::query_as(
        "SELECT id, name, created_at, last_seen, last_version, checks, downloads FROM devices
         WHERE project_id = ? ORDER BY name",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    let latest = store::latest_release(&state.db, project_id, false).await?;
    let mut updates = updates(&state.db, project_id).await?;
    let first = now() / 86400 - DAYS + 1;
    let counts: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT device_id, at / 86400 AS day, COUNT(*) FROM device_events
         WHERE project_id = ? AND at >= ? GROUP BY device_id, day",
    )
    .bind(project_id)
    .bind(first * 86400)
    .fetch_all(&state.db)
    .await?;
    for row in &mut rows {
        row.up_to_date = latest
            .as_ref()
            .is_some_and(|(l, _)| !version::update_available(l, row.last_version.as_deref()));
        row.update = updates.remove(&row.id);
        row.activity = vec![0; DAYS as usize];
        for (_, day, n) in counts.iter().filter(|c| c.0 == row.id) {
            if let Some(slot) = row.activity.get_mut((day - first) as usize) {
                *slot = *n;
            }
        }
    }
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct DeviceInput {
    name: String,
    /// The device's Ed25519 public key (hex). The device makes its own key pair, so the private key never leaves it.
    public_key: String,
}

pub async fn create(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    Json(input): Json<DeviceInput>,
) -> Result<Json<Value>> {
    let name = input.name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(status(StatusCode::BAD_REQUEST, "name is required"));
    }
    let id = format!("dev_{}", hex::encode(rand::random::<[u8; 8]>()));
    let public_key = hex::decode(input.public_key.trim())
        .ok()
        .filter(|key| ParsedPublicKey::new(&ED25519, key).is_ok())
        .ok_or_else(|| {
            status(
                StatusCode::BAD_REQUEST,
                "public_key must be an Ed25519 key in hex",
            )
        })?;
    let done = sqlx::query(
        "INSERT INTO devices (id, project_id, name, public_key, created_at)
         SELECT ?, id, ?, ?, ? FROM projects WHERE id = ?",
    )
    .bind(&id)
    .bind(name)
    .bind(hex::encode(public_key))
    .bind(now())
    .bind(project_id)
    .execute(&state.db)
    .await?;
    if done.rows_affected() == 0 {
        return Err(status(StatusCode::NOT_FOUND, "no such project"));
    }
    Ok(Json(json!({ "id": id })))
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode> {
    sqlx::query("DELETE FROM devices WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
