use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    error::{Result, status},
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
}

pub async fn list(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<DeviceRow>>> {
    let rows = sqlx::query_as(
        "SELECT id, name, created_at, last_seen, last_version, checks, downloads FROM devices
         WHERE project_id = ? ORDER BY name",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
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
