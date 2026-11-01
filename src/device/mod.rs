//! The API devices call. `auth` checks the signature on every request; `handlers` answer.

mod auth;
mod handlers;

use axum::{Router, middleware, routing::get};

#[cfg(test)]
pub use auth::message;
pub use handlers::serve;

use crate::state::AppState;

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/{project}/latest", get(handlers::latest))
        .route("/{project}/releases/{tag}/{asset}", get(handlers::download))
        .route_layer(middleware::from_fn_with_state(state, auth::authenticate))
}
