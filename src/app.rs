//! The HTTP surface: devices under `/api/v1/firmware`, the install page's version picker under `/api/v1/install`, the admin page under `/admin`, the install
//! page at `/firmware/{slug}`, and the UI's files at the root.

use axum::{
    Router,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::Redirect,
    routing::get,
};
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

use crate::{admin, device, install, state::AppState};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::to("/admin") }))
        .route("/health", get(health))
        // `nest` does not match the trailing slash.
        .route("/admin/", get(|| async { Redirect::permanent("/admin") }))
        .nest("/api/v1/firmware", device::router(state.clone()))
        .nest("/api/v1/install", install::router())
        .nest("/admin", admin::router(state.clone(), &state.ui_dir))
        .route_service(
            "/firmware/{slug}",
            ServeFile::new(state.ui_dir.join("index.html")),
        )
        // The UI's scripts, styles and images; anything else is a 404.
        .fallback_service(ServeDir::new(&state.ui_dir))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Independent of GitHub and Clerk, so an outage there does not restart the container.
async fn health(State(state): State<AppState>) -> StatusCode {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
