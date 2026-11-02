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

const FIXTURES: [&str; 7] = [
    "INSERT INTO projects (id, slug, name, repo, assets, first_flash, created_at) VALUES (1, 'ab98', 'AB98', 'o/ab98', 'firmware.bin', 'bootloader.bin@0x0', 0)",
    "INSERT INTO projects (id, slug, name, repo, assets, created_at) VALUES (2, 'cam', 'Cam', 'o/cam', 'firmware.bin', 0)",
    "INSERT INTO releases (id, project_id, tag, prerelease, notes, published_at) VALUES (1, 1, 'v1.2.0', 0, 'n', '')",
    "INSERT INTO releases (id, project_id, tag, prerelease, notes, published_at) VALUES (2, 1, 'v1.3.0-rc1', 1, 'n', '')",
    "INSERT INTO assets (id, release_id, name, github_id, size, sha256, md5) VALUES (1, 1, 'firmware.bin', 77, 5, 'aa', 'bb')",
    "INSERT INTO assets (id, release_id, name, github_id, size, sha256, md5) VALUES (2, 1, 'bootloader.bin', 78, 4, 'cc', 'dd')",
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
    std::fs::write(cache_dir.join("78.bin"), b"boot").unwrap();
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
    let clerk = admin::Clerk::new(
        "sk".into(),
        pk,
        "beacon".into(),
        vec!["https://beacon.test".into()],
    )
    .unwrap();
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
    // The first-flash bootloader is never offered as an update.
    let assets = json["latest"]["assets"].as_array().unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(
        assets[0]["url"],
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
    // A private project's device id alone is not enough.
    let mut req = get(latest);
    req.headers_mut()
        .insert("x-beacon-device", "dev_1".parse().unwrap());
    assert_eq!(call(&state, req).await.0, StatusCode::UNAUTHORIZED);
    // Each signed request works once.
    let ts = now();
    assert_eq!(
        call(&state, signed(latest, KEY, ts)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&state, signed(latest, KEY, ts)).await.0,
        StatusCode::UNAUTHORIZED
    );
    // A device key only opens its own project.
    let other = signed("/api/v1/firmware/cam/latest", KEY, now());
    assert_eq!(call(&state, other).await.0, StatusCode::FORBIDDEN);
    std::fs::remove_dir_all(dir).unwrap();
}

fn with_id(path: &str, id: &str) -> Request<Body> {
    let mut req = get(path);
    req.headers_mut()
        .insert("x-beacon-device", id.parse().unwrap());
    req
}

#[tokio::test]
async fn public_project_needs_only_an_id() {
    let (state, dir) = setup().await;
    sqlx::query("UPDATE projects SET public = 1 WHERE id = 1")
        .execute(&state.db)
        .await
        .unwrap();
    let latest = "/api/v1/firmware/ab98/latest?current=v1.1.0";
    let (code, _, body) = call(&state, with_id(latest, "dev_1")).await;
    assert_eq!(code, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["device"], "dev_1");
    // First-flash files are as open as the rest.
    for name in ["firmware.bin", "bootloader.bin"] {
        let path = format!("/api/v1/firmware/ab98/releases/v1.2.0/{name}");
        assert_eq!(
            call(&state, with_id(&path, "dev_1")).await.0,
            StatusCode::OK
        );
    }
    // Still traced: an unknown id or none on a download is refused.
    assert_eq!(
        call(&state, with_id(latest, "dev_nope")).await.0,
        StatusCode::UNAUTHORIZED
    );
    let download = "/api/v1/firmware/ab98/releases/v1.2.0/firmware.bin";
    assert_eq!(
        call(&state, get(download)).await.0,
        StatusCode::UNAUTHORIZED
    );
    // A board without an id gets one from its first check, and uses it from then on.
    let (code, _, body) = call(&state, get(latest)).await;
    assert_eq!(code, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let id = json["device"].as_str().unwrap();
    assert_ne!(id, "dev_1");
    let (name, version): (String, Option<String>) =
        sqlx::query_as("SELECT name, last_version FROM devices WHERE id = ?")
            .bind(id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(
        (name.as_str(), version.as_deref()),
        ("unnamed", Some("v1.1.0"))
    );
    assert_eq!(call(&state, with_id(download, id)).await.0, StatusCode::OK);
    // A private project issues none.
    assert_eq!(
        call(&state, get("/api/v1/firmware/cam/latest")).await.0,
        StatusCode::UNAUTHORIZED
    );
    // The install page sees the first-flash files and where they go.
    let (_, _, body) = call(&state, get("/api/v1/install/ab98")).await;
    let project: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(project["public"], true);
    assert_eq!(project["first_flash"], "bootloader.bin@0x0");
    assert_eq!(
        project["releases"][0]["assets"][0]["name"],
        "bootloader.bin"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn admin_routes() {
    let (state, dir) = setup().await;
    for path in [
        "/admin/api/projects",
        "/admin/api/projects/1/releases/v1.2.0/bootloader.bin",
    ] {
        assert_eq!(call(&state, get(path)).await.0, StatusCode::UNAUTHORIZED);
    }
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

#[tokio::test]
async fn hostile_input_is_refused_without_panicking() {
    let (state, dir) = setup().await;
    let latest = "/api/v1/firmware/ab98/latest";
    // `now - ts` would overflow these; `now + i64::MIN` used to wrap into the window.
    for ts in [i64::MIN, i64::MAX, now().wrapping_add(i64::MIN)] {
        assert_eq!(
            call(&state, signed(latest, KEY, ts)).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    // Stored with every check, so its length is capped.
    let long = format!("{latest}?current={}", "1".repeat(65));
    assert_eq!(
        call(&state, signed(&long, KEY, now())).await.0,
        StatusCode::BAD_REQUEST
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn public_ids_are_capped_per_minute() {
    let (state, dir) = setup().await;
    sqlx::query("UPDATE projects SET public = 1 WHERE id = 1")
        .execute(&state.db)
        .await
        .unwrap();
    for i in 0..29 {
        sqlx::query(
            "INSERT INTO devices (id, project_id, name, created_at) VALUES (?, 1, 'unnamed', ?)",
        )
        .bind(format!("dev_x{i}"))
        .bind(now())
        .execute(&state.db)
        .await
        .unwrap();
    }
    let latest = "/api/v1/firmware/ab98/latest";
    assert_eq!(call(&state, get(latest)).await.0, StatusCode::OK); // the 30th
    assert_eq!(
        call(&state, get(latest)).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    // A known device is not a new one.
    assert_eq!(
        call(&state, with_id(latest, "dev_1")).await.0,
        StatusCode::OK
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn latest_uses_github_digest_before_caching() {
    let (state, dir) = setup().await;
    // Not cached, and no GitHub to fetch it from: `latest` must not need it.
    sqlx::query("UPDATE assets SET sha256 = NULL, md5 = NULL, digest = 'ee' WHERE github_id = 77")
        .execute(&state.db)
        .await
        .unwrap();
    std::fs::remove_file(state.cache_dir.join("77.bin")).unwrap();
    let (code, _, body) = call(&state, signed("/api/v1/firmware/ab98/latest", KEY, now())).await;
    assert_eq!(code, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let asset = &json["latest"]["assets"][0];
    assert_eq!(
        (&asset["sha256"], &asset["md5"]),
        (&"ee".into(), &serde_json::Value::Null)
    );
    assert_eq!(asset["size"], 5);
    std::fs::remove_dir_all(dir).unwrap();
}
