//! The public install page's version picker (`/firmware/{slug}` in the UI). Everything else the page
//! does goes through the device API, signed by the board over USB (`beacon sign` in the firmware), so
//! only a registered device of the project ever gets its firmware.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde_json::{Value, json};

use crate::{
    error::{Result, status},
    firmware::store,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/{slug}", get(project))
}

/// The project and its releases (tags and asset sizes, no files).
async fn project(State(state): State<AppState>, Path(slug): Path<String>) -> Result<Json<Value>> {
    let (name, emoji, id): (String, String, i64) =
        sqlx::query_as("SELECT name, emoji, id FROM projects WHERE slug = ?")
            .bind(&slug)
            .fetch_optional(&state.db)
            .await?
            .ok_or(status(StatusCode::NOT_FOUND, "no such project"))?;
    let latest = store::latest_release(&state.db, id, false)
        .await?
        .map(|(_, r)| r.tag);
    let mut releases = Vec::new();
    for r in store::releases(&state.db, id).await? {
        let assets: Vec<Value> = store::release_assets(&state.db, r.id)
            .await?
            .into_iter()
            .map(|a| json!({ "name": a.name, "size": a.size }))
            .collect();
        releases.push(json!({ "tag": r.tag, "prerelease": r.prerelease, "published_at": r.published_at, "assets": assets }));
    }
    Ok(Json(
        json!({ "slug": slug, "name": name, "emoji": emoji, "latest": latest, "releases": releases }),
    ))
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request, http::StatusCode};

    use crate::tests::{call, setup};

    #[tokio::test]
    async fn lists_releases_without_files() {
        let (state, dir) = setup().await;
        let get = |path: &str| Request::get(path).body(Body::empty()).unwrap();
        let (code, _, body) = call(&state, get("/api/v1/install/ab98")).await;
        assert_eq!(code, StatusCode::OK);
        let project: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(project["latest"], "v1.2.0");
        let release = &project["releases"][0];
        assert_eq!(
            release["assets"][0],
            serde_json::json!({ "name": "firmware.bin", "size": 5 })
        );
        assert_eq!(
            call(&state, get("/api/v1/install/nope")).await.0,
            StatusCode::NOT_FOUND
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
