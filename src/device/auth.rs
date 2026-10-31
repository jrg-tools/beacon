//! Every device request is signed with that device's own key.
//!
//! Headers: `X-Beacon-Device: <id>`, `X-Beacon-Timestamp: <unix seconds>`,
//! `X-Beacon-Signature: hex(HMAC-SHA256(secret, "<timestamp>\n<METHOD>\n<path?query>"))`.

use axum::{
    extract::{OriginalUri, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::{
    error::{Result, status},
    state::{AppState, now},
};

/// How far a device clock may drift; devices set theirs over NTP.
const MAX_SKEW_SECS: i64 = 300;

/// The signed-in device, put in the request extensions for the handlers.
#[derive(Clone, sqlx::FromRow)]
pub struct Device {
    pub id: String,
    pub project_id: i64,
    pub slug: String,
    pub repo: String,
    secret: String,
}

impl Device {
    /// A device's key only opens its own project.
    pub fn check_project(&self, project: &str) -> Result<()> {
        if self.slug == project {
            Ok(())
        } else {
            Err(status(
                StatusCode::FORBIDDEN,
                "device not registered for this project",
            ))
        }
    }
}

pub fn signature(secret: &str, timestamp: i64, method: &str, path_and_query: &str) -> Hmac<Sha256> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes any key length");
    mac.update(format!("{timestamp}\n{method}\n{path_and_query}").as_bytes());
    mac
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

pub async fn authenticate(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    // One answer for every failure, so a caller cannot probe which device ids exist.
    let denied = || status(StatusCode::UNAUTHORIZED, "invalid signature");
    let headers = req.headers();
    let (Some(id), Some(timestamp), Some(sig)) = (
        header(headers, "x-beacon-device"),
        header(headers, "x-beacon-timestamp").and_then(|t| t.parse::<i64>().ok()),
        header(headers, "x-beacon-signature").and_then(|s| hex::decode(s).ok()),
    ) else {
        return Err(denied());
    };
    if (now() - timestamp).abs() > MAX_SKEW_SECS {
        return Err(status(
            StatusCode::UNAUTHORIZED,
            "timestamp out of range, check the device clock",
        ));
    }
    let device: Device = sqlx::query_as(
        "SELECT d.id, d.project_id, p.slug, p.repo, d.secret FROM devices d
         JOIN projects p ON p.id = d.project_id WHERE d.id = ?",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(denied)?;
    // The full path the device signed, not the one left after `nest` strips the prefix.
    let uri = req
        .extensions()
        .get::<OriginalUri>()
        .map_or(req.uri(), |o| &o.0);
    let path = uri.path_and_query().map_or("/", |p| p.as_str());
    // verify_slice compares in constant time.
    signature(&device.secret, timestamp, req.method().as_str(), path)
        .verify_slice(&sig)
        .map_err(|_| denied())?;
    req.extensions_mut().insert(device);
    Ok(next.run(req).await)
}
