CREATE TABLE projects (
    id INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    -- GitHub "owner/name"; may be private, read with GITHUB_TOKEN.
    repo TEXT NOT NULL,
    -- Comma-separated release asset names devices may download, e.g. "firmware.bin,firmware-filesystem.bin".
    assets TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    synced_at INTEGER,
    sync_error TEXT
);

CREATE TABLE releases (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    prerelease INTEGER NOT NULL,
    notes TEXT NOT NULL,
    published_at TEXT NOT NULL,
    UNIQUE (project_id, tag)
);

CREATE TABLE assets (
    id INTEGER PRIMARY KEY,
    release_id INTEGER NOT NULL REFERENCES releases(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    -- Also the cache file name: a re-uploaded asset gets a new id, so a new file.
    github_id INTEGER NOT NULL,
    size INTEGER NOT NULL,
    -- NULL until the file is in the local cache.
    sha256 TEXT,
    md5 TEXT,
    UNIQUE (release_id, name)
);

CREATE TABLE devices (
    -- What the device sends in X-Beacon-Device.
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    -- The HMAC key; kept in clear because the server must recompute signatures.
    secret TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen INTEGER,
    last_version TEXT
);
