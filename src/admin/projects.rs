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
    firmware::{cache, store, sync},
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
    let mut out = Vec::new();
    for r in store::releases(&state.db, id).await? {
        let assets = store::release_assets(&state.db, r.id).await?;
        out.push(json!({ "tag": r.tag, "prerelease": r.prerelease, "published_at": r.published_at, "assets": assets }));
    }
    Ok(Json(out))
}
