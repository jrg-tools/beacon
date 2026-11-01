# Beacon

Firmware update server for jrg-tools devices (mayanhcu, AB98, …). It mirrors GitHub releases (private
repos included) into SQLite, caches their assets on disk, and serves them to devices that sign every
request. An admin UI behind Clerk manages projects and devices.

- `src/` — the Rust server (axum + sqlx/SQLite). It owns every API and serves the admin UI.
- `web/` — the admin UI (SvelteKit 3, static build, served by Rust under `/admin`).

## Layout

| Module                   | Job                                                                 |
| ------------------------ | ------------------------------------------------------------------- |
| `app.rs`                 | Router: `/api/v1/firmware` (devices), `/admin` (UI + admin API)     |
| `config.rs`, `state.rs`  | Environment settings; shared state                                  |
| `device/auth.rs`         | Ed25519 signature check on every device request                     |
| `device/handlers.rs`     | `latest` and `download`                                             |
| `firmware/version.rs`    | Semver: which release is newest, whether an update is available     |
| `firmware/store.rs`      | Release and asset queries                                           |
| `firmware/cache.rs`      | Assets on disk, fetched from GitHub on first use                    |
| `firmware/sync.rs`       | GitHub → SQLite mirror (every `SYNC_INTERVAL_SECS`, default 2 days, and "Sync now")   |
| `github.rs`              | GitHub REST client                                                  |
| `admin/clerk.rs`         | Clerk session tokens (JWKS) and roles                               |
| `admin/projects.rs`, `admin/devices.rs` | Admin API                                            |

## Registering a device

Plug the board in over USB, enter a name on the project page and click **Sync device** (Web Serial,
so Chrome or Edge on a computer). The page talks to the firmware one line at a time, and the
firmware's replies start with `beacon:` amid its log:

```
> beacon keygen <nonce>     < beacon:pubkey <nonce> <public key hex>
> beacon id <dev_...>       < beacon:ok <dev_...>
```

`keygen` makes a new key pair on the board and stores the private key; a resend with the same nonce
gets the same key, so the page retries safely if opening the port reset the board. The page then
registers the public key (`POST /admin/api/projects/{id}/devices {name, public_key}`, admin only)
and sends the device the id it gets back. Firmware side: `src/network/BeaconSerial.cpp` in AB98 and
mayanhcu.

## Device API

Every request carries three headers, signed with the device's Ed25519 private key. The device makes
that key itself and never shares it; the server keeps only the public key.

```
X-Beacon-Device:    dev_0123456789abcdef
X-Beacon-Timestamp: 1793412161                       # unix seconds, ±5 min: set the clock over NTP
X-Beacon-Signature: hex(Ed25519(private key, "<timestamp>\n<METHOD>\n<path?query>"))
```

The signature covers the exact path and query sent. A wrong signature, unknown device or stale clock
returns `401`; another project's path returns `403`.

`GET /api/v1/firmware/{project}/latest?current=v1.2.3[&channel=beta]` (`beta` also offers prereleases):

```json
{
  "project": "ab98",
  "current": "v1.2.3",
  "update_available": true,
  "latest": {
    "tag": "v1.3.0", "version": "1.3.0", "prerelease": false,
    "published_at": "2026-10-30T10:00:00Z", "notes": "…",
    "assets": [
      { "name": "firmware.bin", "size": 1310288, "sha256": "…", "md5": "…",
        "url": "/api/v1/firmware/ab98/releases/v1.3.0/firmware.bin" }
    ]
  }
}
```

`GET {url}` streams the asset with `Content-Length` and `x-md5`, which ESP32's `HTTPUpdate` checks
before booting the image. Both calls record the device's last contact and version for the admin UI.

### ESP32 (Arduino)

The existing `OtaUpdater` needs three changes: point it at Beacon instead of the GitHub API, read
`latest.assets[].url`, and sign each request in the header callback it already passes to `HTTPUpdate`:

```cpp
#include <Ed25519.h>  // rweather/Crypto

// BEACON_PRIVATE_KEY is the 64 hex characters from the admin UI, decoded to 32 bytes.
String beaconSignature(const uint8_t privateKey[32], long ts, const char* method, const String& pathQuery) {
  const String message = String(ts) + "\n" + method + "\n" + pathQuery;
  uint8_t publicKey[32], sig[64];
  Ed25519::derivePublicKey(publicKey, privateKey);
  Ed25519::sign(sig, privateKey, publicKey, message.c_str(), message.length());
  char hex[129];
  for (int i = 0; i < 64; i++) sprintf(hex + 2 * i, "%02x", sig[i]);
  return String(hex);
}

void addBeaconHeaders(HTTPClient* http, const String& pathQuery) {
  const long ts = time(nullptr);  // configTime() must have synced
  http->addHeader("X-Beacon-Device", BEACON_DEVICE_ID);
  http->addHeader("X-Beacon-Timestamp", String(ts));
  http->addHeader("X-Beacon-Signature", beaconSignature(BEACON_PRIVATE_KEY, ts, "GET", pathQuery));
}
```

## Admin

`/admin` is protected by Clerk. Use the same instance as jrg-life, and give admins `privateMetadata`
`{ "role": "admin", "scope": "…,beacon" }`. A project is a GitHub repo plus the asset names devices
download (for example `firmware.bin,firmware-filesystem.bin` for AB98, or
`mayanhcu-firmware.bin,mayanhcu-filesystem.bin` for `jrg-tools/mayanhcu-releases`). Only semver tags
with at least one of those assets count as releases. The newest stable release is cached right after
each sync, and each sync deletes the cached files of releases that are neither the newest (stable or
beta) nor the version a device last reported; a device asking for one again gets it from GitHub.

## Run

```sh
make run    # first time: creates .env from .env.example; add the Clerk keys and run it again
make dev    # server + UI with hot reload (open the Vite URL + /admin)
make test   # every check CI runs
make help   # the rest
```

`make` loads `.env` itself, so it works from any shell, and falls back to `gh auth token` when
`GITHUB_TOKEN` is unset. The UI is rebuilt only when `web/` changes; the server listens on `BIND`
(default http://localhost:8080/admin).

Docker: `docker run -p 8080:8080 -v beacon:/data --env-file .env ghcr.io/jrg-tools/beacon`. The image is
`scratch` plus a static binary and the UI. CI (`.github/workflows/docker-publish.yml`) runs fmt, clippy
and tests for Rust, and lint, typecheck and build for the UI, then publishes to GHCR from `main` and `v*`
tags.
