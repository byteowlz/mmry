-- Add sparse_embedding column for SPLADE++ neural sparse embeddings
ALTER TABLE memories ADD COLUMN sparse_embedding BLOB;
