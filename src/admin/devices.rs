use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::projects::DAYS;
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
}

/// The secret is returned once, here; the page tells the admin to flash it now.
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
    let secret = hex::encode(rand::random::<[u8; 32]>());
    let done = sqlx::query(
        "INSERT INTO devices (id, project_id, name, secret, created_at)
         SELECT ?, id, ?, ?, ? FROM projects WHERE id = ?",
    )
    .bind(&id)
    .bind(name)
    .bind(&secret)
    .bind(now())
    .bind(project_id)
    .execute(&state.db)
    .await?;
    if done.rows_affected() == 0 {
        return Err(status(StatusCode::NOT_FOUND, "no such project"));
    }
    Ok(Json(json!({ "id": id, "secret": secret })))
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode> {
    sqlx::query("DELETE FROM devices WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
