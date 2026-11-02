//! The install page's version picker (`/firmware/{slug}` in the UI). A public project's is open to
//! anyone, first-flash files included; a private project's only to an admin (without its first-flash
//! files, which the admin API serves): anyone else gets the same 404 as an unknown slug, so the
//! project's name and releases stay private. A private project's owner still updates their board: the
//! page asks the device API's `latest` as the board, signed over USB (`beacon sign` in the firmware),
//! so only a registered device of it ever gets its firmware.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde_json::{Value, json};

use crate::{
    admin,
    error::{Result, status},
    firmware::store,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/{slug}", get(project))
}

/// The project and its releases (tags and asset sizes, no files).
async fn project(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let not_found = || status(StatusCode::NOT_FOUND, "no such project");
    let (name, emoji, id, mut first_flash, public): (String, String, i64, String, bool) =
        sqlx::query_as("SELECT name, emoji, id, first_flash, public FROM projects WHERE slug = ?")
            .bind(&slug)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(not_found)?;
    if !public {
        admin::check_admin(&state, &headers)
            .await
            .map_err(|_| not_found())?;
    }
    // A private project's first-flash files go only through the admin API.
    let hidden = if public {
        String::new()
    } else {
        std::mem::take(&mut first_flash)
    };
    let latest = store::latest_release(&state.db, id, false)
        .await?
        .map(|(_, r)| r.tag);
    let assets = store::project_assets(&state.db, id).await?;
    let mut releases = Vec::new();
    for r in store::releases(&state.db, id).await? {
        let assets: Vec<Value> = assets
            .iter()
            .filter(|a| a.release_id == r.id)
            .filter(|a| !store::first_flash_names(&hidden).any(|n| n == a.name))
            .map(
                |a| json!({ "name": a.name, "size": a.size, "chip": a.chip, "app_sha": a.app_sha }),
            )
            .collect();
        releases.push(json!({ "tag": r.tag, "prerelease": r.prerelease, "published_at": r.published_at, "assets": assets }));
    }
    Ok(Json(
        json!({ "slug": slug, "name": name, "emoji": emoji, "public": public, "first_flash": first_flash, "latest": latest, "releases": releases }),
    ))
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request, http::StatusCode};

    use crate::tests::{call, setup};

    #[tokio::test]
    async fn private_projects_only_for_admins() {
        let (state, dir) = setup().await;
        let get = |path: &str| Request::get(path).body(Body::empty()).unwrap();
        // A private project looks exactly like an unknown one, without an admin's session.
        let private = call(&state, get("/api/v1/install/ab98")).await;
        let unknown = call(&state, get("/api/v1/install/nope")).await;
        assert_eq!(private.0, StatusCode::NOT_FOUND);
        assert_eq!((private.0, private.2), (unknown.0, unknown.2));
        state.clerk.trust_test_admin("user_admin").await;
        let as_user = |user: &str, azp: &str| {
            let token = state.clerk.test_token(user, azp);
            Request::get("/api/v1/install/ab98")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap()
        };
        // Another app's token, even an admin's, is not enough.
        let other_app = call(&state, as_user("user_admin", "https://other.test")).await;
        assert_eq!(other_app.0, StatusCode::NOT_FOUND);

        let (code, _, body) = call(&state, as_user("user_admin", "https://beacon.test")).await;
        assert_eq!(code, StatusCode::OK);
        let project: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(project["latest"], "v1.2.0");
        // Its first-flash files are the admin API's.
        assert_eq!(project["first_flash"], "");
        assert_eq!(
            project["releases"][0]["assets"],
            serde_json::json!([{ "name": "firmware.bin", "size": 5, "chip": null, "app_sha": null }])
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
