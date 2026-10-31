//! The HTTP surface: devices under `/api/v1/firmware`, the admin page under `/admin`.

use axum::{
    Router,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::Redirect,
    routing::get,
};
use tower_http::{set_header::SetResponseHeaderLayer, trace::TraceLayer};

use crate::{admin, device, state::AppState};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::to("/admin") }))
        .route("/health", get(health))
        // `nest` does not match the trailing slash.
        .route("/admin/", get(|| async { Redirect::permanent("/admin") }))
        .nest("/api/v1/firmware", device::router(state.clone()))
        .nest("/admin", admin::router(state.clone(), &state.ui_dir))
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
