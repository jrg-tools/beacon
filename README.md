# Beacon

Firmware update server for jrg-tools devices (mayanhcu, AB98, …). It mirrors GitHub releases (private
repos included) into SQLite, caches their assets on disk, and serves them to devices that sign every
request. An admin UI behind Clerk manages projects and devices.

- `src/` — the Rust server (axum + sqlx/SQLite). It owns every API and serves the admin UI.
- `web/` — the admin UI (SvelteKit 3, static build, served by Rust: admin pages under `/admin`, the install page at `/firmware/<slug>`).

## Layout

| Module                   | Job                                                                 |
| ------------------------ | ------------------------------------------------------------------- |
| `app.rs`                 | Router: `/api/v1/firmware` (devices), `/admin` (UI + admin API); security headers and CSP |
| `config.rs`, `state.rs`  | Environment settings; shared state                                  |
| `device/auth.rs`         | Device auth: Ed25519 signatures (private), ids only (public)        |
| `device/handlers.rs`     | `latest` and `download`                                             |
| `install.rs`             | The install page's version picker (public projects, or an admin)   |
| `firmware/version.rs`    | Semver: which release is newest, whether an update is available     |
| `firmware/store.rs`      | Release and asset queries                                           |
| `firmware/cache.rs`      | Assets on disk, fetched from GitHub on first use, and served        |
| `firmware/sync.rs`       | GitHub → SQLite mirror (every `SYNC_INTERVAL_SECS`, default 2 days, and "Sync now")   |
| `github.rs`              | GitHub REST client                                                  |
| `admin/clerk.rs`         | Clerk session tokens (JWKS) and roles                               |
| `admin/projects.rs`, `admin/devices.rs`, `admin/stats.rs` | Admin API                          |

## Public and private projects

A project is **private** unless **Public** is ticked in its edit form. A private project's devices are
registered with **Sync device** and sign every request; its first-flash files go only to an admin. A
public project's devices sign nothing and need no sync: each sends only its id, and gets one from
Beacon the first time it asks (see [Device API](#device-api)). Its first-flash files are open too.
Switching a project from public to private leaves its devices without keys: sync them.

## Registering a device

Private projects only. Plug the board in over USB, enter a name on the project page and click
**Sync device** (Web Serial, so Chrome or Edge on a computer). The page talks to the firmware one line
at a time, and the firmware's replies start with `beacon:` amid its log:

```
> beacon keygen <nonce>     < beacon:pubkey <nonce> <public key hex>
> beacon id <dev_...>       < beacon:ok <dev_...>
```

`keygen` makes a new key pair on the board and stores the private key; a resend with the same nonce
gets the same key, so the page retries safely if opening the port reset the board. The page then
registers the public key (`POST /admin/api/projects/{id}/devices {name, public_key}`, admin only)
and sends the device the id it gets back. Firmware side: `src/network/BeaconSerial.cpp` in AB98 and
mayanhcu.

## Installing over USB

**Install** on the project page opens `/firmware/<slug>` (the project page is `/admin/project/<slug>`).
It needs no sign-in and loads no Clerk script.

- **Public project**: pick a version (`GET /api/v1/install/{slug}`: tags, asset sizes and the
  first-flash files), plug the board in and click **Connect and install**.
- **Private project**: `GET /api/v1/install/{slug}` is a `404`, the same as for an unknown slug, unless
  the caller is an admin (Clerk session in `Authorization: Bearer`), so nobody else learns its name or
  releases. The page shows one **Connect and update** button: the board signs a `latest` check and
  gets the newest release for it, whose tag shows only then. **Admin sign-in** on the page loads Clerk;
  an admin then gets the version picker and the first flash.

Everything else goes through the device API below, signed by the board over USB for a private project,
so only a registered device of it gets its firmware:

```
> beacon info                          < beacon:info <dev_...|none> <version>
> beacon sign <ts> GET <path?query>    < beacon:sig <ts> GET <path?query> <signature hex|none>
> beacon partitions                    < beacon:partitions <type,subtype,offset hex,size hex,label;...>
```

A public project's board signs nothing (no `beacon sign`): the page sends its id alone.
`beacon sign` signs the same message the board signs for its own requests (only `GET`s of
`/api/v1/firmware/`), and the page sends it with the usual headers: `latest?current=<version>` checks
the board is registered here (and records its version), then each asset is downloaded as a device
download. The page flashes them with esptool-js, using the partition table the app reported
(esptool-js's `readFlash` stalls on the ESP32-S3's USB port): the app to factory/ota_0, the filesystem
image to the SPIFFS/LittleFS partition, and otadata is blanked so the new app boots. NVS (WiFi, the
Beacon key and id) is kept. The board is reset out of the bootloader even when an install fails.
Afterwards the page reopens the port, checks the same device answers on the new version and makes a
signed `latest` check, so the update shows as succeeded. The computer's clock must be within 5 minutes
of the server's, like the board's. Boards on firmware older than `beacon sign` need one update over
the air or with PlatformIO first.

### First flash

A blank or bricked board has no app to answer, so the install page can also do a first flash. The
project's **First flash only** field lists release files at fixed offsets, e.g.
`bootloader.bin@0x0, partitions.bin@0x8000` (the bootloader goes at `0x1000` on the original ESP32).
They are mirrored like the assets but never sent as an update (not in `latest`). The page reads the
partition table from whichever first-flash file is one, places the app and filesystem with it as an
install does, erases the whole flash, writes everything and waits for `beacon info`. Then:

- **Private project**: only an admin, after **Admin sign-in**, sees the
  first-flash files and a **First flash (erase everything)** checkbox; unchecked, it is a normal
  update. The files come through `GET /admin/api/projects/{id}/releases/{tag}/{asset}` (admin only,
  not a device download; a device download of one is `401`). Once the board boots, the page asks for a
  name and registers it as **Sync device** does.
- **Public project**: no checkbox. A board that doesn't answer `beacon info` (blank or other firmware)
  or answers with no id gets a first flash, once the visitor confirms the erase (it wipes the board's
  settings): the page asks `latest` without an id for a new one,
  downloads every file as that device, flashes, and sends the id with `beacon id`. A board with an id
  gets a normal update.

## Device API

On a private project, every request carries three headers, signed with the device's Ed25519 private
key. The device makes that key itself and never shares it; the server keeps only the public key.

```
X-Beacon-Device:    dev_0123456789abcdef
X-Beacon-Timestamp: 1793412161                       # unix seconds, ±5 min: set the clock over NTP
X-Beacon-Signature: hex(Ed25519(private key, "<timestamp>\n<METHOD>\n<path?query>"))
```

The signature covers the exact path and query sent, and works once: the same request again in the
same second is a replay. A wrong or reused signature, unknown device or stale clock returns `401`;
another project's path returns `403`.

On a **public** project a device sends only `X-Beacon-Device` (no timestamp, no signature); an
unknown id is `401`. A device with no id yet calls `latest` without the header: Beacon creates a device
named `unnamed` and returns its id as `device` in the reply. The device stores it (over USB: `beacon id
<id>`) and sends it from then on; any other request without an id is `401`. At most 30 new ids per
project per minute (`429` beyond); an id that is a week old and was never used again (one check, no
download) is deleted.

`GET /api/v1/firmware/{project}/latest?current=v1.2.3[&channel=beta]` (`beta` also offers prereleases;
`current` is at most 64 characters, else `400`):

```json
{
  "project": "ab98",
  "device": "dev_0123456789abcdef",
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

`sha256` is the cached file's, or before it is cached the digest GitHub reports for the asset (the
download is then checked against it), so `latest` does not wait on GitHub; only an asset GitHub has no
digest for (uploaded before mid-2025) is downloaded first. `md5` is `null` until the file is cached.

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

For a public project, send only `X-Beacon-Device` (skip it while the device has no id) and save the
`device` field of `latest`'s reply when it differs from the stored id.

## Admin

`/admin` is protected by Clerk. Use the same instance as jrg-life, and give admins `privateMetadata`
`{ "role": "admin", "scope": "…,beacon" }`. A project is a GitHub repo plus the asset names devices
download (for example `firmware.bin,firmware-filesystem.bin` for AB98, or
`mayanhcu-firmware.bin,mayanhcu-filesystem.bin` for `jrg-tools/mayanhcu-releases`). Only semver tags
with at least one of those assets count as releases. The newest stable release is cached right after
each sync, and each sync deletes the cached files of releases that are neither the newest (stable or
beta) nor the version a device last reported; a device asking for one again gets it from GitHub.

## Security headers

Every response carries a Content-Security-Policy (the UI's files, the inline bootstrap SvelteKit
writes into `index.html` by its SHA-256, Google Fonts, and the Clerk instance's frontend API, its
images and Cloudflare's captcha), `frame-ancestors 'none'` with `X-Frame-Options: DENY`, HSTS,
`nosniff` and a referrer policy. The hash is read from the build when the server starts, so restart
it after rebuilding the UI. Set `CLERK_AUTHORIZED_PARTIES` to Beacon's origins so tokens Clerk issued
to other apps on the same instance are refused.

## Run

```sh
make run    # first time: creates .env from .env.example; add the Clerk keys and run it again
make dev    # server + UI with hot reload (open the Vite URL + /admin)
make test   # every check CI runs
make help   # the rest
```

`make` loads `.env` itself, so it works from any shell. Private repos need `GITHUB_TOKEN` there: a
fine-grained token with only "Contents: read" on them. The UI is rebuilt only when `web/` changes; the server listens on `BIND`
(default http://localhost:8080/admin).

Docker: `docker run -p 8080:8080 -v beacon:/data --env-file .env ghcr.io/jrg-tools/beacon`. The image is
`scratch` plus a static binary and the UI. CI (`.github/workflows/docker-publish.yml`, actions pinned by commit, base images by digest) runs fmt,
clippy, tests and `cargo audit` (rustsec/audit-check) for Rust, and lint, typecheck, `node --test`
(`web/test/flash.test.js`), `pnpm audit` and build for the UI, then publishes to GHCR from `main` and `v*`
tags.
