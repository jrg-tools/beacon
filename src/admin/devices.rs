use aws_lc_rs::signature::{ED25519, ParsedPublicKey};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::stats::{DAYS, UpdateStatus, updates};
use crate::{
    error::{Result, status},
    firmware::{store, version},
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

pub async fn list(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<DeviceRow>>> {
    let mut rows: Vec<DeviceRow> = sqlx::query_as(
        "SELECT id, name, created_at, last_seen, last_version, checks, downloads FROM devices
         WHERE project_id = ? ORDER BY name",
    )
    .bind(&project_id)
    .fetch_all(&state.db)
    .await?;
    let latest = store::latest_release(&state.db, &project_id, false).await?;
    let mut updates = updates(&state.db, &project_id).await?;
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
    Path(project_id): Path<String>,
    Json(input): Json<DeviceInput>,
) -> Result<Json<Value>> {
    let name = valid_name(&input.name)?;
    let id = crate::device::new_id();
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
         SELECT ?, id, ?, ?, ? FROM projects WHERE id = ? AND NOT public",
    )
    .bind(&id)
    .bind(name)
    .bind(hex::encode(public_key))
    .bind(now())
    .bind(project_id)
    .execute(&state.db)
    .await?;
    if done.rows_affected() == 0 {
        // A public project's devices get their id unsigned (`device::handlers`); there is no key to sync.
        return Err(status(StatusCode::NOT_FOUND, "no such private project"));
    }
    Ok(Json(json!({ "id": id })))
}

fn valid_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(status(StatusCode::BAD_REQUEST, "name is required"));
    }
    Ok(name)
}

#[derive(Deserialize)]
pub struct RenameInput {
    name: String,
}

/// Mostly for a public project's devices, which start out "unnamed".
pub async fn rename(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RenameInput>,
) -> Result<StatusCode> {
    let done = sqlx::query("UPDATE devices SET name = ? WHERE id = ?")
        .bind(valid_name(&input.name)?)
        .bind(id)
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(status(StatusCode::NOT_FOUND, "no such device"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode> {
    sqlx::query("DELETE FROM devices WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
