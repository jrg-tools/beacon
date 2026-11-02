-- Public projects: devices send only their id, unsigned, and anyone may install or first-flash.
-- Existing projects stay private.
ALTER TABLE projects ADD COLUMN public INTEGER NOT NULL DEFAULT 0;
