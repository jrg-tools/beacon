//! The HTTP surface: devices under `/api/v1/firmware`, the install page's version picker under `/api/v1/install`, the admin page under `/admin`, the install
//! page at `/firmware/{slug}`, and the UI's files at the root.

use std::path::Path;

use axum::{
    Router,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::Redirect,
    routing::get,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
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
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            csp(&state.ui_dir, &state.clerk.frontend_api),
        ))
        // For browsers older than `frame-ancestors`: the install page drives a USB board.
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        // Ignored over plain HTTP (local runs); behind TLS, the browser sticks to HTTPS.
        .layer(SetResponseHeaderLayer::overriding(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// The UI's own files, the inline bootstrap SvelteKit writes into index.html (by hash, read from the
/// build at startup, so a rebuild needs a restart), Google Fonts (app.html) and Clerk: its scripts
/// from the instance's frontend API, its images, and the Cloudflare captcha its sign-in may show.
fn csp(ui_dir: &Path, clerk: &str) -> HeaderValue {
    let html = std::fs::read_to_string(ui_dir.join("index.html")).unwrap_or_default();
    let hashes: String = html
        .split("<script>")
        .skip(1)
        .filter_map(|rest| rest.split_once("</script>"))
        .map(|(js, _)| format!(" 'sha256-{}'", STANDARD.encode(Sha256::digest(js))))
        .collect();
    let policy = format!(
        "default-src 'self'; script-src 'self'{hashes} https://{clerk} https://challenges.cloudflare.com; \
         connect-src 'self' https://{clerk} https://clerk-telemetry.com; \
         img-src 'self' data: https://img.clerk.com; \
         style-src 'self' 'unsafe-inline'; \
         font-src 'self'; worker-src 'self' blob:; \
         frame-src https://challenges.cloudflare.com; frame-ancestors 'none'; \
         base-uri 'self'; form-action 'self'; object-src 'none'"
    );
    // `clerk` is a host name (checked in `Clerk::new`) and the hashes are base64.
    HeaderValue::from_str(&policy).expect("a valid header")
}

/// Independent of GitHub and Clerk, so an outage there does not restart the container.
async fn health(State(state): State<AppState>) -> StatusCode {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};

    use crate::tests::{call, setup};

    #[tokio::test]
    async fn security_headers_on_every_response() {
        let (state, dir) = setup().await;
        // As SvelteKit's build writes it; the policy allows exactly this inline script.
        std::fs::write(
            state.ui_dir.join("index.html"),
            "<!doctype html><script>boot()</script>admin",
        )
        .unwrap();
        for path in ["/firmware/ab98", "/api/v1/install/nope", "/health"] {
            let (_, headers, _) =
                call(&state, Request::get(path).body(Body::empty()).unwrap()).await;
            let csp = headers["content-security-policy"].to_str().unwrap();
            // echo -n 'boot()' | openssl dgst -sha256 -binary | base64
            assert!(csp.contains("'sha256-MeZS89WlF0u+o0hCvHTBt4q1WHU+U+sJKgbdRUc36mY='"));
            assert!(csp.contains("script-src 'self' 'sha256-"));
            assert!(csp.contains("https://x.clerk.accounts.dev"));
            assert!(csp.contains("frame-ancestors 'none'"));
            assert_eq!(headers["x-frame-options"], "DENY");
            assert_eq!(headers["strict-transport-security"], "max-age=31536000");
            assert_eq!(headers["x-content-type-options"], "nosniff");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
