//! Mirrors a project's GitHub releases into SQLite, then warms the cache.

use std::{collections::HashSet, time::Duration};

use sqlx::{Sqlite, Transaction};

use super::{cache, parse_version, store};
use crate::{github, state::AppState, state::now};

#[derive(sqlx::FromRow)]
struct Project {
    id: i64,
    repo: String,
    assets: String,
    first_flash: String,
}

/// Syncs one project and records the outcome on it, so the admin page can show failures.
pub async fn project(state: &AppState, project_id: i64) -> anyhow::Result<()> {
    let _guard = state.sync_lock.lock().await;
    let project: Project =
        sqlx::query_as("SELECT id, repo, assets, first_flash FROM projects WHERE id = ?")
            .bind(project_id)
            .fetch_one(&state.db)
            .await?;
    let result = run(state, &project).await;
    if let Err(e) = &result {
        tracing::warn!(repo = project.repo, "sync failed: {e:#}");
    }
    sqlx::query("UPDATE projects SET synced_at = ?, sync_error = ? WHERE id = ?")
        .bind(now())
        .bind(result.as_ref().err().map(|e| format!("{e:#}")))
        .bind(project.id)
        .execute(&state.db)
        .await?;
    result
}

/// Runs in the background in the admin API: GitHub and first downloads can take a while.
pub fn spawn(state: AppState, project_id: i64) {
    tokio::spawn(async move {
        let _ = project(&state, project_id).await; // logged and stored on the project
    });
}

/// Syncs every project now and then; a failing project does not stop the others.
pub async fn every(state: AppState, period: Duration) {
    let mut tick = tokio::time::interval(period);
    loop {
        tick.tick().await;
        if let Err(e) = prune(&state).await {
            tracing::error!("pruning: {e}");
        }
        match sqlx::query_as::<_, (i64,)>("SELECT id FROM projects")
            .fetch_all(&state.db)
            .await
        {
            Ok(ids) => {
                for (id,) in ids {
                    let _ = project(&state, id).await;
                }
            }
            Err(e) => tracing::error!("listing projects: {e}"),
        }
    }
}

/// Events older than 90 days, and public ids handed out a week ago and never used again
/// (anyone can ask for one; a real board checks again or downloads).
async fn prune(state: &AppState) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM device_events WHERE at < ?")
        .bind(now() - 90 * 86400)
        .execute(&state.db)
        .await?;
    sqlx::query(
        "DELETE FROM devices WHERE public_key = '' AND name = 'unnamed' AND checks <= 1
           AND downloads = 0 AND created_at < ?",
    )
    .bind(now() - 7 * 86400)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn run(state: &AppState, project: &Project) -> anyhow::Result<()> {
    let wanted: HashSet<&str> = project
        .assets
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let first_flash: HashSet<&str> = store::first_flash_names(&project.first_flash).collect();
    let releases = state.github.releases(&project.repo).await?;

    let mut tx = state.db.begin().await?;
    let mut kept_tags = Vec::new();
    for release in &releases {
        let assets: Vec<_> = release
            .assets
            .iter()
            .filter(|a| wanted.contains(a.name.as_str()) || first_flash.contains(a.name.as_str()))
            .collect();
        // Only a release with something to update to counts, not one with just first-flash files.
        let updatable = assets.iter().any(|a| wanted.contains(a.name.as_str()));
        if release.draft || !updatable || parse_version(&release.tag_name).is_none() {
            continue;
        }
        upsert_release(&mut tx, project.id, release, &assets).await?;
        kept_tags.push(release.tag_name.as_str());
    }
    // Releases deleted on GitHub (or older than the 30 fetched) stop being offered.
    sqlx::query(
        "DELETE FROM releases WHERE project_id = ? AND tag NOT IN (SELECT value FROM json_each(?))",
    )
    .bind(project.id)
    .bind(serde_json::to_string(&kept_tags)?)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    cache::remove_orphans(state).await?;
    cache::remove_unused(state, project.id).await?;
    // Warm the cache with what devices will ask for next.
    if let Some((_, release)) = store::latest_release(&state.db, project.id, false).await? {
        for asset in store::release_assets(&state.db, release.id).await? {
            cache::ensure(state, &project.repo, asset).await?;
        }
    }
    Ok(())
}

async fn upsert_release(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: i64,
    release: &github::Release,
    assets: &[&github::Asset],
) -> anyhow::Result<()> {
    let (release_id,): (i64,) = sqlx::query_as(
        "INSERT INTO releases (project_id, tag, prerelease, notes, published_at) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (project_id, tag) DO UPDATE SET
           prerelease = excluded.prerelease, notes = excluded.notes, published_at = excluded.published_at
         RETURNING id",
    )
    .bind(project_id)
    .bind(&release.tag_name)
    .bind(release.prerelease)
    .bind(release.body.as_deref().unwrap_or_default())
    .bind(release.published_at.as_deref().unwrap_or_default())
    .fetch_one(&mut **tx)
    .await?;
    for asset in assets {
        // A re-uploaded asset has a new GitHub id: forget the old hashes.
        sqlx::query(
            "INSERT INTO assets (release_id, name, github_id, size, digest) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (release_id, name) DO UPDATE SET
               sha256 = CASE WHEN github_id = excluded.github_id THEN sha256 END,
               md5 = CASE WHEN github_id = excluded.github_id THEN md5 END,
               github_id = excluded.github_id, size = excluded.size, digest = excluded.digest",
        )
        .bind(release_id)
        .bind(&asset.name)
        .bind(asset.id)
        .bind(asset.size)
        .bind(asset.sha256())
        .execute(&mut **tx)
        .await?;
    }
    let names: Vec<&str> = assets.iter().map(|a| a.name.as_str()).collect();
    sqlx::query(
        "DELETE FROM assets WHERE release_id = ? AND name NOT IN (SELECT value FROM json_each(?))",
    )
    .bind(release_id)
    .bind(serde_json::to_string(&names)?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{state::now, tests::setup};

    #[tokio::test]
    async fn prunes_ids_never_used_again() {
        let (state, dir) = setup().await;
        let old = now() - 8 * 86400;
        for (id, checks, downloads) in [("dev_junk", 1, 0), ("dev_used", 1, 1), ("dev_back", 2, 0)]
        {
            sqlx::query(
                "INSERT INTO devices (id, project_id, name, created_at, checks, downloads)
                 VALUES (?, 2, 'unnamed', ?, ?, ?)",
            )
            .bind(id)
            .bind(old)
            .bind(checks)
            .bind(downloads)
            .execute(&state.db)
            .await
            .unwrap();
        }
        super::prune(&state).await.unwrap();
        let ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM devices ORDER BY id")
            .fetch_all(&state.db)
            .await
            .unwrap();
        let ids: Vec<_> = ids.into_iter().map(|(id,)| id).collect();
        // dev_1 is a private device: it has a key.
        assert_eq!(ids, ["dev_1", "dev_back", "dev_used"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
