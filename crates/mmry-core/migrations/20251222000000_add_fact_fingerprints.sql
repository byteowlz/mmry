-- Add fact fingerprints for deduplication.
-- Note: existing facts are backfilled and de-duplicated in Rust (Database::apply_schema_updates).

ALTER TABLE facts ADD COLUMN fact_fingerprint TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS idx_facts_fingerprint ON facts(fact_fingerprint);
