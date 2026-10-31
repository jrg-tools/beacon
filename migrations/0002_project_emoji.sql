-- Shown on the project's tile in the admin UI.
ALTER TABLE projects ADD COLUMN emoji TEXT NOT NULL DEFAULT '📡';
