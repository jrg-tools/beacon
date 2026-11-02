//! Reads of releases and assets shared by the device and admin APIs.

use semver::Version;
use serde::Serialize;
use sqlx::SqlitePool;

use super::version::pick_latest;

#[derive(sqlx::FromRow, Clone)]
pub struct ReleaseRow {
    pub id: i64,
    pub project_id: i64,
    pub tag: String,
    pub prerelease: bool,
    pub notes: String,
    pub published_at: String,
}

#[derive(sqlx::FromRow, Serialize, Clone)]
pub struct AssetRow {
    #[serde(skip)]
    pub id: i64,
    #[serde(skip)]
    pub release_id: i64,
    pub name: String,
    #[serde(skip)]
    pub github_id: i64,
    pub size: i64,
    pub sha256: Option<String>,
    pub md5: Option<String>,
    /// GitHub's SHA-256 of the file, known before it is cached (not for older uploads).
    #[serde(skip)]
    pub digest: Option<String>,
    /// From the file's header once cached (`image`): the chip it is built for and, for an app,
    /// its ELF SHA-256, which the install page matches against what a board runs.
    pub chip: Option<i64>,
    pub app_sha: Option<String>,
}

/// An asset query: every `AssetRow` column, then `$rest` (sqlx wants a literal).
macro_rules! assets {
    ($rest:literal) => {
        concat!(
            "SELECT a.id, a.release_id, a.name, a.github_id, a.size, a.sha256, a.md5, a.digest, a.chip, a.app_sha FROM assets a ",
            $rest
        )
    };
}

/// The file names in a project's `first_flash` ("name@0xoffset,…").
pub fn first_flash_names(first_flash: &str) -> impl Iterator<Item = &str> {
    first_flash
        .split(',')
        .filter_map(|entry| entry.split_once('@'))
        .map(|(name, _)| name)
}

pub async fn releases(db: &SqlitePool, project_id: i64) -> sqlx::Result<Vec<ReleaseRow>> {
    sqlx::query_as(
        "SELECT id, project_id, tag, prerelease, notes, published_at FROM releases
         WHERE project_id = ? ORDER BY published_at DESC",
    )
    .bind(project_id)
    .fetch_all(db)
    .await
}

/// Every project's releases, for listing them all without a query per project.
pub async fn all_releases(db: &SqlitePool) -> sqlx::Result<Vec<ReleaseRow>> {
    sqlx::query_as(
        "SELECT id, project_id, tag, prerelease, notes, published_at FROM releases ORDER BY published_at DESC",
    )
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
    sqlx::query_as(assets!("WHERE a.release_id = ? ORDER BY a.name"))
        .bind(release_id)
        .fetch_all(db)
        .await
}

/// Every asset of the project's releases in one query; pick a release's out by `release_id`.
pub async fn project_assets(db: &SqlitePool, project_id: i64) -> sqlx::Result<Vec<AssetRow>> {
    sqlx::query_as(assets!(
        "JOIN releases r ON r.id = a.release_id WHERE r.project_id = ? ORDER BY a.name"
    ))
    .bind(project_id)
    .fetch_all(db)
    .await
}

pub async fn asset(db: &SqlitePool, id: i64) -> sqlx::Result<AssetRow> {
    sqlx::query_as(assets!("WHERE a.id = ?"))
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
    sqlx::query_as(assets!(
        "JOIN releases r ON r.id = a.release_id WHERE r.project_id = ? AND r.tag = ? AND a.name = ?"
    ))
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

pub async fn set_image(
    db: &SqlitePool,
    id: i64,
    chip: Option<i64>,
    app_sha: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE assets SET chip = ?, app_sha = ? WHERE id = ?")
        .bind(chip)
        .bind(app_sha)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// An ESP image's chip (bytes 12-13 of its header) and, for an app built with an
/// `esp_app_desc_t`, its ELF SHA-256 (at 0xb0, where esptool's `--elf-sha256-offset` puts it).
/// Both `None` for anything else, a filesystem image or a partition table.
pub fn image(head: &[u8]) -> (Option<i64>, Option<String>) {
    if head.len() < 24 || head[0] != 0xe9 {
        return (None, None);
    }
    let chip = i64::from(u16::from_le_bytes([head[12], head[13]]));
    let app_sha = head
        .get(0xb0..0xd0)
        .filter(|_| head[32..36] == 0xabcd5432u32.to_le_bytes())
        .filter(|sha| sha.iter().any(|&b| b != 0))
        .map(hex::encode);
    (Some(chip), app_sha)
}

#[cfg(test)]
mod tests {
    #[test]
    fn image_reads_chip_and_app_hash() {
        let mut app = vec![0u8; 256];
        app[0] = 0xe9;
        app[12] = 9; // ESP32-S3
        app[32..36].copy_from_slice(&0xabcd5432u32.to_le_bytes());
        app[0xb0] = 0xab;
        let (chip, sha) = super::image(&app);
        assert_eq!(chip, Some(9));
        assert_eq!(sha.unwrap(), format!("ab{}", "00".repeat(31)));
        // A bootloader: a chip, no app description.
        app[32] = 0;
        assert_eq!(super::image(&app), (Some(9), None));
        assert_eq!(super::image(b"littlefs"), (None, None));
    }
}
