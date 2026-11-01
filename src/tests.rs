//! The HTTP API end to end, against a real SQLite file and a pre-filled cache (no GitHub).

use std::path::PathBuf;

use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use base64::Engine;
use tower::ServiceExt;

use crate::{
    admin, app, db, device, github,
    state::{AppState, State, now},
};

const FIXTURES: [&str; 6] = [
    "INSERT INTO projects (id, slug, name, repo, assets, created_at) VALUES (1, 'ab98', 'AB98', 'o/ab98', 'firmware.bin', 0)",
    "INSERT INTO projects (id, slug, name, repo, assets, created_at) VALUES (2, 'cam', 'Cam', 'o/cam', 'firmware.bin', 0)",
    "INSERT INTO releases (id, project_id, tag, prerelease, notes, published_at) VALUES (1, 1, 'v1.2.0', 0, 'n', '')",
    "INSERT INTO releases (id, project_id, tag, prerelease, notes, published_at) VALUES (2, 1, 'v1.3.0-rc1', 1, 'n', '')",
    "INSERT INTO assets (release_id, name, github_id, size, sha256, md5) VALUES (1, 'firmware.bin', 77, 5, 'aa', 'bb')",
    "INSERT INTO devices (id, project_id, name, created_at) VALUES ('dev_1', 1, 'desk', 0)",
];

/// dev_1's private key.
pub(crate) const KEY: [u8; 32] = [1; 32];

pub(crate) async fn setup() -> (AppState, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "beacon-test-{}",
        hex::encode(rand::random::<[u8; 8]>())
    ));
    let (cache_dir, ui_dir) = (dir.join("cache"), dir.join("ui"));
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::create_dir_all(&ui_dir).unwrap();
    std::fs::write(cache_dir.join("77.bin"), b"hello").unwrap();
    std::fs::write(ui_dir.join("index.html"), "<!doctype html>admin").unwrap();
    let db = db::open(&dir.join("t.db")).await.unwrap();
    for sql in FIXTURES {
        sqlx::query(sql).execute(&db).await.unwrap();
    }
    sqlx::query("UPDATE devices SET public_key = ? WHERE id = 'dev_1'")
        .bind(hex::encode(pair(KEY).public_key()))
        .execute(&db)
        .await
        .unwrap();
    let http = reqwest::Client::new();
    let pk = format!(
        "pk_test_{}",
        base64::engine::general_purpose::STANDARD.encode("x.clerk.accounts.dev$")
    );
    let clerk = admin::Clerk::new("sk".into(), pk, "beacon".into()).unwrap();
    let github = github::GitHub {
        http: http.clone(),
        token: None,
    };
    (State::new(db, http, github, clerk, cache_dir, ui_dir), dir)
}

fn pair(key: [u8; 32]) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&key).unwrap()
}

pub(crate) fn signed(path: &str, key: [u8; 32], ts: i64) -> Request<Body> {
    let sig = hex::encode(pair(key).sign(device::message(ts, "GET", path).as_bytes()));
    Request::get(path)
        .header("x-beacon-device", "dev_1")
        .header("x-beacon-timestamp", ts.to_string())
        .header("x-beacon-signature", sig)
        .body(Body::empty())
        .unwrap()
}

fn get(path: &str) -> Request<Body> {
    Request::get(path).body(Body::empty()).unwrap()
}

pub(crate) async fn call(state: &AppState, req: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let res = app::router(state.clone()).oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    let body = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (parts.status, parts.headers, body)
}

#[tokio::test]
async fn device_api() {
    let (state, dir) = setup().await;

    let (code, _, body) = call(
        &state,
        signed("/api/v1/firmware/ab98/latest?current=v1.1.0", KEY, now()),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["update_available"], true);
    assert_eq!(json["latest"]["tag"], "v1.2.0"); // the rc is beta-only
    assert_eq!(
        json["latest"]["assets"][0]["url"],
        "/api/v1/firmware/ab98/releases/v1.2.0/firmware.bin"
    );
    let (seen,): (Option<String>,) =
        sqlx::query_as("SELECT last_version FROM devices WHERE id = 'dev_1'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(seen.as_deref(), Some("v1.1.0"));

    let (_, _, body) = call(
        &state,
        signed("/api/v1/firmware/ab98/latest?channel=beta", KEY, now()),
    )
    .await;
    assert!(String::from_utf8(body).unwrap().contains("v1.3.0-rc1"));

    let path = "/api/v1/firmware/ab98/releases/v1.2.0/firmware.bin";
    let (code, headers, body) = call(&state, signed(path, KEY, now())).await;
    assert_eq!(
        (code, body.as_slice()),
        (StatusCode::OK, b"hello".as_slice())
    );
    assert_eq!(headers["x-md5"], "bb");
    let counters: (i64, i64) =
        sqlx::query_as("SELECT checks, downloads FROM devices WHERE id = 'dev_1'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(counters, (2, 1));

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn device_api_rejects_bad_signatures() {
    let (state, dir) = setup().await;
    let latest = "/api/v1/firmware/ab98/latest";
    assert_eq!(
        call(&state, signed(latest, [2; 32], now())).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&state, signed(latest, KEY, now() - 3600)).await.0,
        StatusCode::UNAUTHORIZED
    );
    // A signature is only good for the exact path and query it was made for.
    let mut req = signed(latest, KEY, now());
    *req.uri_mut() = format!("{latest}?channel=beta").parse().unwrap();
    assert_eq!(call(&state, req).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&state, get(latest)).await.0, StatusCode::UNAUTHORIZED);
    // A device key only opens its own project.
    let other = signed("/api/v1/firmware/cam/latest", KEY, now());
    assert_eq!(call(&state, other).await.0, StatusCode::FORBIDDEN);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn admin_routes() {
    let (state, dir) = setup().await;
    assert_eq!(
        call(&state, get("/admin/api/projects")).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&state, get("/admin/api/nope")).await.0,
        StatusCode::NOT_FOUND
    );
    let (code, _, body) = call(&state, get("/admin/api/config")).await;
    assert_eq!(code, StatusCode::OK);
    assert!(String::from_utf8(body).unwrap().contains("pk_test_"));
    // The SPA: its index for the root and for client-side routes.
    for path in [
        "/admin",
        "/admin/project/ab98",
        "/firmware/ab98",
        "/index.html",
    ] {
        let (code, _, body) = call(&state, get(path)).await;
        assert_eq!((path, code), (path, StatusCode::OK));
        assert!(String::from_utf8(body).unwrap().contains("admin"));
    }
    // Only the UI's pages fall back to it.
    assert_eq!(call(&state, get("/nope")).await.0, StatusCode::NOT_FOUND);
    let (code, headers, _) = call(&state, get("/admin/")).await;
    assert_eq!(
        (code, headers["location"].to_str().unwrap()),
        (StatusCode::PERMANENT_REDIRECT, "/admin")
    );
    std::fs::remove_dir_all(dir).unwrap();
}
