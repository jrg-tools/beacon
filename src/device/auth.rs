//! Every device request is signed with that device's own key.
//!
//! Headers: `X-Beacon-Device: <id>`, `X-Beacon-Timestamp: <unix seconds>`,
//! `X-Beacon-Signature: hex(Ed25519(private key, "<timestamp>\n<METHOD>\n<path?query>"))`.
//! The server stores only the public key.

use aws_lc_rs::signature::{ED25519, UnparsedPublicKey};
use axum::{
    extract::{OriginalUri, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};

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
    public_key: String,
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

/// The bytes a device signs.
pub fn message(timestamp: i64, method: &str, path_and_query: &str) -> String {
    format!("{timestamp}\n{method}\n{path_and_query}")
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
        "SELECT d.id, d.project_id, p.slug, p.repo, d.public_key FROM devices d
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
    let key = hex::decode(&device.public_key).map_err(|_| denied())?;
    UnparsedPublicKey::new(&ED25519, key)
        .verify(
            message(timestamp, req.method().as_str(), path).as_bytes(),
            &sig,
        )
        .map_err(|_| denied())?;
    req.extensions_mut().insert(device);
    Ok(next.run(req).await)
}
