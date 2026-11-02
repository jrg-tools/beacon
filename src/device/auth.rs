//! Every device request of a private project is signed with that device's own key.
//!
//! Headers: `X-Beacon-Device: <id>`, `X-Beacon-Timestamp: <unix seconds>`,
//! `X-Beacon-Signature: hex(Ed25519(private key, "<timestamp>\n<METHOD>\n<path?query>"))`.
//! The server stores only the public key.
//!
//! A public project's devices send only `X-Beacon-Device`, and one without an id gets a new one
//! from its first `latest` (see `issue`).

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
    pub first_flash: String,
    pub public: bool,
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

const DEVICE: &str =
    "SELECT d.id, d.project_id, p.slug, p.repo, p.first_flash, p.public, d.public_key
    FROM devices d JOIN projects p ON p.id = d.project_id WHERE d.id = ?";

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
    let Some(id) = header(headers, "x-beacon-device") else {
        // Only a public project's `latest` may come without an id (after `nest`: "/{project}/latest").
        let slug = req.uri().path().strip_suffix("/latest").unwrap_or("");
        let device = issue(&state, slug.trim_start_matches('/')).await?;
        req.extensions_mut().insert(device.ok_or_else(denied)?);
        return Ok(next.run(req).await);
    };
    let device: Device = sqlx::query_as(DEVICE)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(denied)?;
    // Unsigned: a public project's firmware is open, the id only traces the device.
    if device.public {
        req.extensions_mut().insert(device);
        return Ok(next.run(req).await);
    }
    let (Some(timestamp), Some(sig)) = (
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
    // Ed25519 signs a message the same way every time, so a signature seen within the window is a
    // replay. Only valid ones are kept, so junk can't fill the map.
    // shortcut: in memory, so a restart forgets the last 5 minutes; move to the db if that matters.
    {
        let mut seen = state.seen_signatures.lock().unwrap();
        let at = now();
        seen.retain(|_, ts| (at - *ts).abs() <= MAX_SKEW_SECS);
        if seen.insert(sig, timestamp).is_some() {
            return Err(status(StatusCode::UNAUTHORIZED, "request already used"));
        }
    }
    req.extensions_mut().insert(device);
    Ok(next.run(req).await)
}

/// A new device of a public project, named "unnamed"; `None` if the
/// project is private or unknown. `latest` returns its id, which the device keeps from then on.
async fn issue(state: &AppState, slug: &str) -> Result<Option<Device>> {
    // A cap per project, not per IP: enough to stop a loop filling the table.
    // shortcut: a flood also blocks real new boards for a minute; key it by IP if that happens.
    let (recent,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM devices d JOIN projects p ON p.id = d.project_id
         WHERE p.slug = ? AND d.created_at > ?",
    )
    .bind(slug)
    .bind(now() - 60)
    .fetch_one(&state.db)
    .await?;
    if recent >= 30 {
        return Err(status(
            StatusCode::TOO_MANY_REQUESTS,
            "too many new devices, try again later",
        ));
    }
    let id = format!("dev_{}", hex::encode(rand::random::<[u8; 8]>()));
    let done = sqlx::query(
        "INSERT INTO devices (id, project_id, name, created_at)
         SELECT ?, id, 'unnamed', ? FROM projects WHERE slug = ? AND public",
    )
    .bind(&id)
    .bind(now())
    .bind(slug)
    .execute(&state.db)
    .await?;
    if done.rows_affected() == 0 {
        return Ok(None);
    }
    Ok(Some(
        sqlx::query_as(DEVICE)
            .bind(&id)
            .fetch_one(&state.db)
            .await?,
    ))
}
