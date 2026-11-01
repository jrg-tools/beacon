-- Usage stats for the admin UI: update checks and asset downloads per device.
ALTER TABLE devices ADD COLUMN checks INTEGER NOT NULL DEFAULT 0;
ALTER TABLE devices ADD COLUMN downloads INTEGER NOT NULL DEFAULT 0;
