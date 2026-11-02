-- UUIDv7 ids (text) instead of integer rowids; the app generates them (`state::uuid`).
-- Existing rows get a v7 from their own time in ms and their old id in the low bits: unique, valid,
-- and in the old order. SQLite can't change a column's type, so every table is rebuilt; old tables
-- are dropped children first, as dropping a parent with foreign keys on deletes its children.

CREATE TABLE projects_new (
    id TEXT PRIMARY KEY NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    -- GitHub "owner/name"; may be private, read with GITHUB_TOKEN.
    repo TEXT NOT NULL,
    -- Comma-separated release asset names devices may download, e.g. "firmware.bin,firmware-filesystem.bin".
    assets TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    synced_at INTEGER,
    sync_error TEXT,
    emoji TEXT NOT NULL DEFAULT '📡',
    first_flash TEXT NOT NULL DEFAULT '',
    public INTEGER NOT NULL DEFAULT 0
);
INSERT INTO projects_new
SELECT printf('%08x-%04x-7000-8000-%012x', (created_at * 1000) >> 16, (created_at * 1000) & 65535, id),
    slug, name, repo, assets, created_at, synced_at, sync_error, emoji, first_flash, public
FROM projects;

CREATE TABLE releases_new (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects_new(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    prerelease INTEGER NOT NULL,
    notes TEXT NOT NULL,
    published_at TEXT NOT NULL,
    UNIQUE (project_id, tag)
);
INSERT INTO releases_new
SELECT printf('%08x-%04x-7000-8000-%012x', (COALESCE(unixepoch(r.published_at), 0) * 1000) >> 16,
        (COALESCE(unixepoch(r.published_at), 0) * 1000) & 65535, r.id),
    pn.id, r.tag, r.prerelease, r.notes, r.published_at
FROM releases r JOIN projects p ON p.id = r.project_id JOIN projects_new pn ON pn.slug = p.slug;

CREATE TABLE assets_new (
    id TEXT PRIMARY KEY NOT NULL,
    release_id TEXT NOT NULL REFERENCES releases_new(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    -- Also the cache file name: a re-uploaded asset gets a new id, so a new file.
    github_id INTEGER NOT NULL,
    size INTEGER NOT NULL,
    -- NULL until the file is in the local cache.
    sha256 TEXT,
    md5 TEXT,
    -- The SHA-256 GitHub reports for the asset (null for assets uploaded before GitHub computed them).
    digest TEXT,
    -- From the cached file's header; kept when the file is evicted (see 0010).
    chip INTEGER,
    app_sha TEXT,
    UNIQUE (release_id, name)
);
INSERT INTO assets_new
SELECT printf('%08x-%04x-7000-8000-%012x', 0, 0, a.id),
    rn.id, a.name, a.github_id, a.size, a.sha256, a.md5, a.digest, a.chip, a.app_sha
FROM assets a JOIN releases r ON r.id = a.release_id JOIN projects p ON p.id = r.project_id
JOIN projects_new pn ON pn.slug = p.slug JOIN releases_new rn ON rn.project_id = pn.id AND rn.tag = r.tag;

CREATE TABLE devices_new (
    -- What the device sends in X-Beacon-Device.
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects_new(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen INTEGER,
    last_version TEXT,
    checks INTEGER NOT NULL DEFAULT 0,
    downloads INTEGER NOT NULL DEFAULT 0,
    public_key TEXT NOT NULL DEFAULT ''
);
INSERT INTO devices_new
SELECT d.id, pn.id, d.name, d.created_at, d.last_seen, d.last_version, d.checks, d.downloads, d.public_key
FROM devices d JOIN projects p ON p.id = d.project_id JOIN projects_new pn ON pn.slug = p.slug;

-- One row per device request; pruned after 90 days. No FK on device_id (see 0004).
CREATE TABLE device_events_new (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects_new(id) ON DELETE CASCADE,
    device_id TEXT NOT NULL,
    at INTEGER NOT NULL,
    -- 'check' or 'download'.
    kind TEXT NOT NULL,
    version TEXT,
    channel TEXT,
    bytes INTEGER,
    tag TEXT
);
INSERT INTO device_events_new
SELECT printf('%08x-%04x-7000-8000-%012x', (e.at * 1000) >> 16, (e.at * 1000) & 65535, e.id),
    pn.id, e.device_id, e.at, e.kind, e.version, e.channel, e.bytes, e.tag
FROM device_events e JOIN projects p ON p.id = e.project_id JOIN projects_new pn ON pn.slug = p.slug;

DROP TABLE device_events;
DROP TABLE devices;
DROP TABLE assets;
DROP TABLE releases;
DROP TABLE projects;

-- Renaming also rewrites the other tables' REFERENCES to the new names.
ALTER TABLE projects_new RENAME TO projects;
ALTER TABLE releases_new RENAME TO releases;
ALTER TABLE assets_new RENAME TO assets;
ALTER TABLE devices_new RENAME TO devices;
ALTER TABLE device_events_new RENAME TO device_events;

CREATE INDEX device_events_project_at ON device_events (project_id, at);
