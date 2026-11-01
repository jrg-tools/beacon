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
}

/// Syncs one project and records the outcome on it, so the admin page can show failures.
pub async fn project(state: &AppState, project_id: i64) -> anyhow::Result<()> {
    let _guard = state.sync_lock.lock().await;
    let project: Project = sqlx::query_as("SELECT id, repo, assets FROM projects WHERE id = ?")
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
        let old = now() - 90 * 86400;
        if let Err(e) = sqlx::query("DELETE FROM device_events WHERE at < ?")
            .bind(old)
            .execute(&state.db)
            .await
        {
            tracing::error!("pruning device events: {e}");
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

async fn run(state: &AppState, project: &Project) -> anyhow::Result<()> {
    let wanted: HashSet<&str> = project
        .assets
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let releases = state.github.releases(&project.repo).await?;

    let mut tx = state.db.begin().await?;
    let mut kept_tags = Vec::new();
    for release in &releases {
        let assets: Vec<_> = release
            .assets
            .iter()
            .filter(|a| wanted.contains(a.name.as_str()))
            .collect();
        if release.draft || assets.is_empty() || parse_version(&release.tag_name).is_none() {
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
            "INSERT INTO assets (release_id, name, github_id, size) VALUES (?, ?, ?, ?)
             ON CONFLICT (release_id, name) DO UPDATE SET
               sha256 = CASE WHEN github_id = excluded.github_id THEN sha256 END,
               md5 = CASE WHEN github_id = excluded.github_id THEN md5 END,
               github_id = excluded.github_id, size = excluded.size",
        )
        .bind(release_id)
        .bind(&asset.name)
        .bind(asset.id)
        .bind(asset.size)
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
