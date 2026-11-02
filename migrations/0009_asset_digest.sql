-- The SHA-256 GitHub reports for the asset (null for assets uploaded before GitHub computed them).
-- `latest` offers it before the file is cached, and the download is checked against it.
ALTER TABLE assets ADD COLUMN digest TEXT;
