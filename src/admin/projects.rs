use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::project_input::ProjectInput;
use crate::{
    error::{AppError, Result, status},
    firmware::{cache, parse_version, store, sync, version},
    state::{AppState, now, uuid},
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
    id: String,
    slug: String,
    name: String,
    emoji: String,
    repo: String,
    assets: String,
    first_flash: String,
    public: bool,
    synced_at: Option<i64>,
    sync_error: Option<String>,
    releases: i64,
    devices: i64,
    #[sqlx(skip)]
    latest: Option<String>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Only this project (the project and install pages open one by slug).
    slug: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ProjectRow>>> {
    let mut rows: Vec<ProjectRow> = sqlx::query_as(
        "SELECT p.id, p.slug, p.name, p.emoji, p.repo, p.assets, p.first_flash, p.public, p.synced_at, p.sync_error,
           (SELECT COUNT(*) FROM releases WHERE project_id = p.id) AS releases,
           (SELECT COUNT(*) FROM devices WHERE project_id = p.id) AS devices
         FROM projects p WHERE p.slug = COALESCE(?, p.slug) ORDER BY p.name",
    )
    .bind(q.slug)
    .fetch_all(&state.db)
    .await?;
    // One query for every project's releases; the newest is picked by semver, not in SQL.
    let releases = store::all_releases(&state.db).await?;
    for row in &mut rows {
        let own = releases.iter().filter(|r| r.project_id == row.id).cloned();
        row.latest = version::pick_latest(own.collect(), false).map(|(_, r)| r.tag);
    }
    Ok(Json(rows))
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<ProjectInput>,
) -> Result<Json<Value>> {
    let p = input.validate()?;
    let id = uuid();
    sqlx::query(
        "INSERT INTO projects (id, slug, name, emoji, repo, assets, first_flash, public, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&p.slug)
    .bind(&p.name)
    .bind(&p.emoji)
    .bind(&p.repo)
    .bind(&p.assets)
    .bind(&p.first_flash)
    .bind(p.public)
    .bind(now())
    .execute(&state.db)
    .await
    .map_err(conflict_or)?;
    sync::spawn(state, id.clone());
    Ok(Json(json!({ "id": id })))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProjectInput>,
) -> Result<StatusCode> {
    let p = input.validate()?;
    let done = sqlx::query(
        "UPDATE projects SET slug = ?, name = ?, emoji = ?, repo = ?, assets = ?, first_flash = ?,
           public = ? WHERE id = ?",
    )
    .bind(&p.slug)
    .bind(&p.name)
    .bind(&p.emoji)
    .bind(&p.repo)
    .bind(&p.assets)
    .bind(&p.first_flash)
    .bind(p.public)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(conflict_or)?;
    if done.rows_affected() == 0 {
        return Err(status(StatusCode::NOT_FOUND, "no such project"));
    }
    sync::spawn(state, id);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode> {
    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    cache::remove_orphans(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn sync(State(state): State<AppState>, Path(id): Path<String>) -> StatusCode {
    sync::spawn(state, id);
    StatusCode::ACCEPTED
}

/// Any file of a release, first-flash ones included: the admin's first flash downloads them here,
/// as a board would from the device API but without counting as a device download.
pub async fn download(
    State(state): State<AppState>,
    Path((id, tag, name)): Path<(String, String, String)>,
) -> Result<axum::response::Response> {
    let (repo,): (String,) = sqlx::query_as("SELECT repo FROM projects WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(status(StatusCode::NOT_FOUND, "no such project"))?;
    let asset = store::find_asset(&state.db, &id, &tag, &name)
        .await?
        .ok_or(status(StatusCode::NOT_FOUND, "no such asset"))?;
    cache::serve(&state, &repo, asset).await
}

pub async fn releases(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Value>>> {
    let devices: Vec<(Option<String>, String)> =
        sqlx::query_as("SELECT last_version, id FROM devices WHERE project_id = ?")
            .bind(&id)
            .fetch_all(&state.db)
            .await?;
    // Compared as semver, so a device reporting `1.2.0` counts for the tag `v1.2.0`.
    let installed: Vec<_> = devices
        .iter()
        .filter_map(|(v, _)| v.as_deref().and_then(parse_version))
        .collect();
    // Only current devices: a revoked one would stay pending forever.
    let mut updates = super::stats::updates(&state.db, &id).await?;
    updates.retain(|device, _| devices.iter().any(|d| &d.1 == device));
    let all_assets = store::project_assets(&state.db, &id).await?;
    let mut out = Vec::new();
    for r in store::releases(&state.db, &id).await? {
        let assets: Vec<_> = all_assets.iter().filter(|a| a.release_id == r.id).collect();
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
