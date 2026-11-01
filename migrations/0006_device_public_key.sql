-- Devices sign with Ed25519: the server keeps only the public key, so the database holds no secret.
-- Devices registered before this have no key and can never sign in, so they go (their events stay);
-- register them again. The default only satisfies SQLite: every insert sets a key.
DELETE FROM devices;
ALTER TABLE devices DROP COLUMN secret;
ALTER TABLE devices ADD COLUMN public_key TEXT NOT NULL DEFAULT '';
