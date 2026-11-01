-- One row per device request, for the project page's charts; pruned after 90 days.
-- No FK on device_id: a revoked device's history stays in its project's charts.
CREATE TABLE device_events (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    device_id TEXT NOT NULL,
    at INTEGER NOT NULL,
    -- 'check' or 'download'.
    kind TEXT NOT NULL,
    -- Checks: the version the device reported and its channel.
    version TEXT,
    channel TEXT,
    -- Downloads: the asset's size.
    bytes INTEGER
);
CREATE INDEX device_events_project_at ON device_events (project_id, at);
