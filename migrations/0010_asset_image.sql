-- Read from the cached file's header: the chip an app or bootloader image is built for (esptool's
-- IMAGE_CHIP_ID) and the app's ELF SHA-256 (esp_app_desc_t), which names the release a board runs.
-- Kept when the file is evicted: they belong to the GitHub upload, like `digest`.
ALTER TABLE assets ADD COLUMN chip INTEGER;
ALTER TABLE assets ADD COLUMN app_sha TEXT;
