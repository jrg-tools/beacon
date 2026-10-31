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
use tower_http::services::{ServeDir, ServeFile};

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
            "/projects/{id}/devices",
            get(devices::list).post(devices::create),
        )
        .route("/devices/{id}", delete(devices::remove))
        .route_layer(middleware::from_fn_with_state(state, auth::require_admin))
        // Public: the page needs the key before anyone can sign in.
        .route("/config", get(config))
        .fallback(|| async { status(StatusCode::NOT_FOUND, "not found") });
    // A single-page app: unknown paths get index.html and the client router takes over.
    let ui = ServeDir::new(ui_dir).fallback(ServeFile::new(ui_dir.join("index.html")));
    Router::new().nest("/api", api).fallback_service(ui)
}

async fn config(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "clerk_publishable_key": state.clerk.publishable_key }))
}
