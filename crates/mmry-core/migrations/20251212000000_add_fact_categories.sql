-- Add category and provenance fields to facts table
-- Based on Sean-V-Dev's HMLR fact extraction system

-- Add category column (Definition, Acronym, Secret, Entity, General)
ALTER TABLE facts ADD COLUMN category TEXT DEFAULT 'General';

-- Add evidence snippet for provenance (10-20 word context)
ALTER TABLE facts ADD COLUMN evidence_snippet TEXT;

-- Add chunk-level provenance columns
ALTER TABLE facts ADD COLUMN source_chunk_id TEXT;
ALTER TABLE facts ADD COLUMN source_paragraph_id TEXT;

-- Create index on category for filtered queries
CREATE INDEX IF NOT EXISTS idx_facts_category ON facts(category);

-- Create index on chunk_id for provenance lookups
CREATE INDEX IF NOT EXISTS idx_facts_chunk ON facts(source_chunk_id);
