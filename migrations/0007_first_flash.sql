-- Release files only the admin's first flash writes, never offered as an update: "name@0xoffset,…".
ALTER TABLE projects ADD COLUMN first_flash TEXT NOT NULL DEFAULT '';
