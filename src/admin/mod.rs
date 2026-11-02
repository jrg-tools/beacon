//! The admin UI (the SvelteKit build in `web/`, served as static files) and the JSON API it calls.

mod auth;
mod clerk;
mod devices;
mod project_input;
mod projects;

use std::path::Path;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    middleware,
    routing::{delete, get, post},
};
use serde_json::{Value, json};
use tower_http::services::ServeFile;

pub use clerk::Clerk;

use crate::{error::status, state::AppState};

pub fn router(state: AppState, ui_dir: &Path) -> Router<AppState> {
    let api = Router::new()
        .route("/projects", get(projects::list).post(projects::create))
        .route(
            "/projects/{id}",
            delete(projects::remove).put(projects::update),
        )
        .route("/projects/{id}/sync", post(projects::sync))
        .route("/projects/{id}/releases", get(projects::releases))
        .route(
            "/projects/{id}/releases/{tag}/{asset}",
            get(projects::download),
        )
        .route("/projects/{id}/stats", get(projects::stats))
        .route(
            "/projects/{id}/devices",
            get(devices::list).post(devices::create),
        )
        .route(
            "/devices/{id}",
            delete(devices::remove).patch(devices::rename),
        )
        .route_layer(middleware::from_fn_with_state(state, auth::require_admin))
        // Public: the page needs the key before anyone can sign in.
        .route("/config", get(config))
        .fallback(|| async { status(StatusCode::NOT_FOUND, "not found") });
    // A single-page app: its pages get index.html and the client router takes over. Its files are
    // served from the root (`app::router`).
    Router::new()
        .nest("/api", api)
        .fallback_service(ServeFile::new(ui_dir.join("index.html")))
}

async fn config(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "clerk_publishable_key": state.clerk.publishable_key }))
}

#[cfg(test)]
mod tests {
    use axum::extract::{Path, State};

    use crate::{
        state::now,
        tests::{KEY, call, setup, signed},
    };

    #[tokio::test]
    async fn stats_follow_device_requests() {
        let (state, dir) = setup().await;
        for path in [
            "/api/v1/firmware/ab98/latest?current=v1.1.0",
            "/api/v1/firmware/ab98/releases/v1.2.0/firmware.bin",
            "/api/v1/firmware/ab98/latest?current=v1.2.0&channel=beta",
        ] {
            call(&state, signed(path, KEY, now())).await;
        }

        let stats = super::projects::stats(State(state.clone()), Path(1))
            .await
            .ok()
            .unwrap()
            .0;
        assert_eq!(stats["devices"], 1);
        assert_eq!(stats["active_24h"], 1);
        assert_eq!(stats["up_to_date"], 1);
        assert_eq!(stats["latest"], "v1.2.0");
        assert_eq!(stats["updates"], 1); // v1.1.0 -> v1.2.0
        assert_eq!(stats["channels"]["beta"], 1);
        assert_eq!(stats["versions"]["v1.2.0"], 1);
        let days = stats["days"].as_array().unwrap();
        assert_eq!(days.len(), super::projects::DAYS as usize);
        let today = days.last().unwrap();
        assert_eq!(
            (today["checks"].clone(), today["downloads"].clone()),
            (2.into(), 1.into())
        );
        assert_eq!(
            (today["bytes"].clone(), today["active"].clone()),
            (5.into(), 1.into())
        );

        let releases = super::projects::releases(State(state.clone()), Path(1))
            .await
            .ok()
            .unwrap()
            .0;
        let mut installed: Vec<_> = releases
            .iter()
            .map(|r| (r["tag"].as_str().unwrap(), r["devices"].as_i64().unwrap()))
            .collect();
        installed.sort(); // both fixtures share a publish date
        assert_eq!(installed, [("v1.2.0", 1), ("v1.3.0-rc1", 0)]);

        let devices = super::devices::list(State(state.clone()), Path(1))
            .await
            .ok()
            .unwrap()
            .0;
        let device = serde_json::to_value(&devices[0]).unwrap();
        assert_eq!(device["up_to_date"], true);
        assert_eq!(device["activity"].as_array().unwrap().last().unwrap(), 3);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn update_status_follows_the_next_check() {
        let (state, dir) = setup().await;
        // A second apart, as a device's are: the same signature twice is a replay.
        let clock = std::sync::atomic::AtomicI64::new(now() - 60);
        let request = |path: &'static str| {
            let state = state.clone();
            let ts = clock.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            async move { call(&state, signed(path, KEY, ts)).await }
        };
        let status = || async {
            let devices = super::devices::list(State(state.clone()), Path(1))
                .await
                .ok()
                .unwrap()
                .0;
            let device = serde_json::to_value(&devices[0]).unwrap();
            let stats = super::projects::stats(State(state.clone()), Path(1))
                .await
                .ok()
                .unwrap()
                .0;
            (
                device["update"]["status"].as_str().map(String::from),
                stats["updates_pending"].as_i64().unwrap(),
                stats["updates_failed"].as_i64().unwrap(),
            )
        };
        let download = "/api/v1/firmware/ab98/releases/v1.2.0/firmware.bin";

        request("/api/v1/firmware/ab98/latest?current=v1.1.0").await;
        assert_eq!(status().await, (None, 0, 0));
        request(download).await;
        assert_eq!(status().await, (Some("pending".into()), 1, 0));
        let releases = super::projects::releases(State(state.clone()), Path(1))
            .await
            .ok()
            .unwrap()
            .0;
        let v120 = releases.iter().find(|r| r["tag"] == "v1.2.0").unwrap();
        assert_eq!(v120["pending"], 1);
        request("/api/v1/firmware/ab98/latest?current=1.2.0").await;
        assert_eq!(status().await, (Some("succeeded".into()), 0, 0));
        // Downloaded again, but it came back on the old version.
        request(download).await;
        request("/api/v1/firmware/ab98/latest?current=v1.1.0").await;
        assert_eq!(status().await, (Some("failed".into()), 0, 1));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn first_flash_downloads_any_file() {
        let (state, dir) = setup().await;
        let download = |name: &str| {
            let path = Path((1, "v1.2.0".to_string(), name.to_string()));
            super::projects::download(State(state.clone()), path)
        };
        let res = download("bootloader.bin").await.ok().unwrap();
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"boot");
        assert!(download("nope.bin").await.is_err());
        // Not a device download: the fleet's counters stay put.
        let (downloads,): (i64,) = sqlx::query_as("SELECT downloads FROM devices")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(downloads, 0);
        // A private project's device gets them only through here, as the admin.
        let path = "/api/v1/firmware/ab98/releases/v1.2.0/bootloader.bin";
        let code = call(&state, signed(path, KEY, now())).await.0;
        assert_eq!(code, axum::http::StatusCode::UNAUTHORIZED);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn registered_key_signs_in() {
        let (state, dir) = setup().await;
        // The device's key pair; only its public half reaches Beacon.
        let key = [7; 32];
        let public_key = aws_lc_rs::signature::Ed25519KeyPair::from_seed_unchecked(&key).unwrap();
        let create = |public_key: String| {
            let input = serde_json::from_value(
                serde_json::json!({ "name": "new", "public_key": public_key }),
            );
            super::devices::create(State(state.clone()), Path(1), axum::Json(input.unwrap()))
        };
        assert!(create("not hex".into()).await.is_err());
        assert!(create("00".into()).await.is_err());
        let created = create(hex::encode(aws_lc_rs::signature::KeyPair::public_key(
            &public_key,
        )))
        .await
        .ok()
        .unwrap()
        .0;
        let mut req = signed("/api/v1/firmware/ab98/latest", key, now());
        let id = created["id"].as_str().unwrap().parse().unwrap();
        req.headers_mut().insert("x-beacon-device", id);
        assert_eq!(call(&state, req).await.0, axum::http::StatusCode::OK);
        // A public project's devices have no key to sync.
        sqlx::query("UPDATE projects SET public = 1 WHERE id = 1")
            .execute(&state.db)
            .await
            .unwrap();
        let public_key = hex::encode(aws_lc_rs::signature::KeyPair::public_key(&public_key));
        assert!(create(public_key).await.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn rename_device() {
        let (state, dir) = setup().await;
        let rename = |id: &str, name: &str| {
            let input = serde_json::from_value(serde_json::json!({ "name": name })).unwrap();
            super::devices::rename(State(state.clone()), Path(id.into()), axum::Json(input))
        };
        assert!(rename("dev_1", "  ").await.is_err());
        assert!(rename("dev_nope", "x").await.is_err());
        assert!(rename("dev_1", " shelf ").await.is_ok());
        let devices = super::devices::list(State(state.clone()), Path(1))
            .await
            .ok()
            .unwrap()
            .0;
        assert_eq!(serde_json::to_value(&devices[0]).unwrap()["name"], "shelf");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
