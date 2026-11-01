//! Reads of releases and assets shared by the device and admin APIs.

use semver::Version;
use serde::Serialize;
use sqlx::SqlitePool;

use super::version::pick_latest;

#[derive(sqlx::FromRow, Clone)]
pub struct ReleaseRow {
    pub id: i64,
    pub tag: String,
    pub prerelease: bool,
    pub notes: String,
    pub published_at: String,
}

#[derive(sqlx::FromRow, Serialize, Clone)]
pub struct AssetRow {
    #[serde(skip)]
    pub id: i64,
    pub name: String,
    #[serde(skip)]
    pub github_id: i64,
    pub size: i64,
    pub sha256: Option<String>,
    pub md5: Option<String>,
}

pub async fn releases(db: &SqlitePool, project_id: i64) -> sqlx::Result<Vec<ReleaseRow>> {
    sqlx::query_as(
        "SELECT id, tag, prerelease, notes, published_at FROM releases
         WHERE project_id = ? ORDER BY published_at DESC",
    )
    .bind(project_id)
    .fetch_all(db)
    .await
}

pub async fn latest_release(
    db: &SqlitePool,
    project_id: i64,
    beta: bool,
) -> sqlx::Result<Option<(Version, ReleaseRow)>> {
    Ok(pick_latest(releases(db, project_id).await?, beta))
}

pub async fn release_assets(db: &SqlitePool, release_id: i64) -> sqlx::Result<Vec<AssetRow>> {
    sqlx::query_as(
        "SELECT a.id, a.name, a.github_id, a.size, a.sha256, a.md5 FROM assets a WHERE a.release_id = ? ORDER BY a.name",
    )
    .bind(release_id)
    .fetch_all(db)
    .await
}

pub async fn asset(db: &SqlitePool, id: i64) -> sqlx::Result<AssetRow> {
    sqlx::query_as(
        "SELECT a.id, a.name, a.github_id, a.size, a.sha256, a.md5 FROM assets a WHERE a.id = ?",
    )
    .bind(id)
    .fetch_one(db)
    .await
}

pub async fn find_asset(
    db: &SqlitePool,
    project_id: i64,
    tag: &str,
    name: &str,
) -> sqlx::Result<Option<AssetRow>> {
    sqlx::query_as(
        "SELECT a.id, a.name, a.github_id, a.size, a.sha256, a.md5 FROM assets a JOIN releases r ON r.id = a.release_id
         WHERE r.project_id = ? AND r.tag = ? AND a.name = ?",
    )
    .bind(project_id)
    .bind(tag)
    .bind(name)
    .fetch_optional(db)
    .await
}

/// Marks the asset as not cached; the next request downloads and hashes it again.
pub async fn clear_hashes(db: &SqlitePool, id: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE assets SET sha256 = NULL, md5 = NULL WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_hashes(db: &SqlitePool, id: i64, sha256: &str, md5: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE assets SET sha256 = ?, md5 = ? WHERE id = ?")
        .bind(sha256)
        .bind(md5)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}
