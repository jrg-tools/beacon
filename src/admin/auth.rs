use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::Response,
};

use crate::{
    error::{Result, status},
    state::AppState,
};

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// Every admin API call: a valid Clerk session whose user has the admin role.
pub async fn require_admin(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response> {
    let token = bearer(req.headers()).ok_or(status(StatusCode::UNAUTHORIZED, "sign in"))?;
    let user_id = state.clerk.verify(&state.http, token).await?;
    if !state.clerk.is_admin(&state.http, &user_id).await? {
        return Err(status(StatusCode::FORBIDDEN, "admin role required"));
    }
    Ok(next.run(req).await)
}
