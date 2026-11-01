-- Downloads: the release tag, so the admin can tell whether the device came back on it.
ALTER TABLE device_events ADD COLUMN tag TEXT;
