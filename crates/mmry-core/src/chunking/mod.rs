use crate::config::ChunkingConfig;
use crate::memory::ChunkMethod;
use crate::memory::Memory;
use crate::Result;
use std::sync::Arc;
use tokenizers::Tokenizer;
use unicode_segmentation::UnicodeSegmentation;

pub struct TextChunk {
    pub content: String,
    pub method: ChunkMethod,
    pub index: usize,
}

pub struct Chunker {
    config: ChunkingConfig,
    tokenizer: Option<Arc<Tokenizer>>,
}

impl Chunker {
    pub fn new(config: ChunkingConfig) -> Self {
        Self {
            config,
            tokenizer: None,
        }
    }

    /// Create a new chunker with a specific tokenizer
    pub fn with_tokenizer(config: ChunkingConfig, tokenizer: Arc<Tokenizer>) -> Self {
        Self {
            config,
            tokenizer: Some(tokenizer),
        }
    }

    /// Count tokens in text using the tokenizer if available, otherwise use word count
    fn count_tokens(&self, text: &str) -> usize {
        if let Some(tokenizer) = &self.tokenizer {
            // Use proper tokenization
            if let Ok(encoding) = tokenizer.encode(text, false) {
                return encoding.len();
            }
        }

        // Fallback to word count (rough approximation)
        // Most tokenizers produce ~1.3x tokens per word on average
        let word_count = text.split_whitespace().count();
        (word_count as f32 * 1.3).ceil() as usize
    }

    /// Check if text needs chunking
    pub fn needs_chunking(&self, text: &str) -> bool {
        if !self.config.enabled {
            return false;
        }
        self.count_tokens(text) > self.config.max_chunk_tokens
    }

    /// Split text into chunks using cascading strategy:
    /// 1. Try paragraphs (double newline)
    /// 2. Fall back to paragraphs (single newline)
    /// 3. Fall back to sentences
    /// 4. Fall back to words
    pub fn chunk_text(&self, text: &str) -> Result<Vec<TextChunk>> {
        if !self.needs_chunking(text) {
            return Ok(vec![TextChunk {
                content: text.to_string(),
                method: ChunkMethod::None,
                index: 0,
            }]);
        }

        // Try paragraph splitting (double newline first)
        if let Ok(chunks) = self.chunk_by_paragraphs(text, "\n\n") {
            if !chunks.is_empty() {
                return Ok(chunks);
            }
        }

        // Fall back to single newline
        if let Ok(chunks) = self.chunk_by_paragraphs(text, "\n") {
            if !chunks.is_empty() {
                return Ok(chunks);
            }
        }

        // Fall back to sentences
        if let Ok(chunks) = self.chunk_by_sentences(text) {
            if !chunks.is_empty() {
                return Ok(chunks);
            }
        }

        // Final fallback to words
        self.chunk_by_words(text)
    }

    /// Split by paragraphs with configurable separator
    fn chunk_by_paragraphs(&self, text: &str, separator: &str) -> Result<Vec<TextChunk>> {
        let paragraphs: Vec<&str> = text
            .split(separator)
            .filter(|p| !p.trim().is_empty())
            .collect();

        if paragraphs.is_empty() {
            return Ok(Vec::new());
        }

        let mut chunks = Vec::new();
        let mut current_chunk = String::new();
        let mut index = 0;

        for paragraph in paragraphs {
            let para_tokens = self.count_tokens(paragraph);

            // If single paragraph exceeds max, it will be handled by sentence/word chunking
            if para_tokens > self.config.max_chunk_tokens {
                // Save current chunk if any
                if !current_chunk.is_empty() {
                    chunks.push(TextChunk {
                        content: current_chunk.trim().to_string(),
                        method: ChunkMethod::Paragraph,
                        index,
                    });
                }
                // This paragraph is too large, abort paragraph chunking
                return Ok(Vec::new());
            }

            let potential_chunk = if current_chunk.is_empty() {
                paragraph.to_string()
            } else {
                format!("{current_chunk}{separator}{paragraph}")
            };

            let potential_tokens = self.count_tokens(&potential_chunk);

            if potential_tokens <= self.config.max_chunk_tokens {
                current_chunk = potential_chunk;
            } else {
                // Save current chunk
                if !current_chunk.is_empty() {
                    chunks.push(TextChunk {
                        content: current_chunk.trim().to_string(),
                        method: ChunkMethod::Paragraph,
                        index,
                    });
                    index += 1;
                }
                current_chunk = paragraph.to_string();
            }
        }

        // Save remaining chunk
        if !current_chunk.is_empty() {
            chunks.push(TextChunk {
                content: current_chunk.trim().to_string(),
                method: ChunkMethod::Paragraph,
                index,
            });
        }

        // Apply overlap if configured
        if self.config.overlap_tokens > 0 && chunks.len() > 1 {
            chunks = self.apply_overlap(chunks);
        }

        Ok(chunks)
    }

    /// Split by sentences using unicode segmentation
    fn chunk_by_sentences(&self, text: &str) -> Result<Vec<TextChunk>> {
        let sentences: Vec<&str> = UnicodeSegmentation::unicode_sentences(text).collect();

        if sentences.is_empty() {
            return Ok(Vec::new());
        }

        let mut chunks = Vec::new();
        let mut current_chunk = String::new();
        let mut index = 0;

        for sentence in sentences {
            let sent_tokens = self.count_tokens(sentence);

            // If single sentence exceeds max, it will be handled by word chunking
            if sent_tokens > self.config.max_chunk_tokens {
                if !current_chunk.is_empty() {
                    chunks.push(TextChunk {
                        content: current_chunk.trim().to_string(),
                        method: ChunkMethod::Sentence,
                        index,
                    });
                }
                return Ok(Vec::new());
            }

            let potential_chunk = if current_chunk.is_empty() {
                sentence.to_string()
            } else {
                format!("{current_chunk} {sentence}")
            };

            let potential_tokens = self.count_tokens(&potential_chunk);

            if potential_tokens <= self.config.max_chunk_tokens {
                current_chunk = potential_chunk;
            } else {
                if !current_chunk.is_empty() {
                    chunks.push(TextChunk {
                        content: current_chunk.trim().to_string(),
                        method: ChunkMethod::Sentence,
                        index,
                    });
                    index += 1;
                }
                current_chunk = sentence.to_string();
            }
        }

        if !current_chunk.is_empty() {
            chunks.push(TextChunk {
                content: current_chunk.trim().to_string(),
                method: ChunkMethod::Sentence,
                index,
            });
        }

        if self.config.overlap_tokens > 0 && chunks.len() > 1 {
            chunks = self.apply_overlap(chunks);
        }

        Ok(chunks)
    }

    /// Split by words (final fallback)
    fn chunk_by_words(&self, text: &str) -> Result<Vec<TextChunk>> {
        let words: Vec<&str> = text.split_whitespace().collect();

        if words.is_empty() {
            return Ok(vec![TextChunk {
                content: text.to_string(),
                method: ChunkMethod::Word,
                index: 0,
            }]);
        }

        let mut chunks = Vec::new();
        let max_words = self.config.max_chunk_tokens.min(words.len());
        let mut index = 0;

        let mut i = 0;
        while i < words.len() {
            let end = (i + max_words).min(words.len());
            let chunk_words = &words[i..end];
            let content = chunk_words.join(" ");

            chunks.push(TextChunk {
                content,
                method: ChunkMethod::Word,
                index,
            });

            index += 1;
            i = end;
        }

        if self.config.overlap_tokens > 0 && chunks.len() > 1 {
            chunks = self.apply_overlap(chunks);
        }

        Ok(chunks)
    }

    /// Apply overlap between chunks
    fn apply_overlap(&self, chunks: Vec<TextChunk>) -> Vec<TextChunk> {
        if chunks.len() < 2 {
            return chunks;
        }

        let overlap_tokens = self.config.overlap_tokens;
        let mut result = Vec::new();

        for i in 0..chunks.len() {
            let mut content = chunks[i].content.clone();

            // Add overlap from previous chunk
            if i > 0 {
                let prev_words: Vec<&str> = chunks[i - 1].content.split_whitespace().collect();
                let overlap_count = overlap_tokens.min(prev_words.len());
                if overlap_count > 0 {
                    let overlap_start = prev_words.len().saturating_sub(overlap_count);
                    let overlap = prev_words[overlap_start..].join(" ");
                    content = format!("{overlap} {content}");
                }
            }

            result.push(TextChunk {
                content,
                method: chunks[i].method.clone(),
                index: i,
            });
        }

        result
    }

    /// Create memory chunks from a parent memory
    pub fn create_memory_chunks(
        &self,
        parent: &Memory,
        text_chunks: Vec<TextChunk>,
    ) -> Vec<Memory> {
        let total_chunks = text_chunks.len() as i32;
        let parent_id = parent.id;

        text_chunks
            .into_iter()
            .map(|chunk| {
                let mut memory = Memory::new(
                    parent.memory_type.clone(),
                    chunk.content,
                    parent.category.clone(),
                );
                memory.parent_id = Some(parent_id);
                memory.chunk_index = Some(chunk.index as i32);
                memory.total_chunks = Some(total_chunks);
                memory.chunk_method = Some(chunk.method);
                memory.importance = parent.importance;
                memory.tags = parent.tags.clone();
                memory.metadata = parent.metadata.clone();
                memory
            })
            .collect()
    }

    /// Generate metadata embedding text from memory metadata
    pub fn generate_metadata_text(&self, memory: &Memory) -> String {
        if !self.config.embed_metadata {
            return String::new();
        }

        let mut parts = Vec::new();

        // Add memory type
        parts.push(format!("Type: {:?}", memory.memory_type));

        // Add category
        if !memory.category.is_empty() {
            parts.push(format!("Category: {}", memory.category));
        }

        // Add tags
        if !memory.tags.is_empty() {
            parts.push(format!("Tags: {}", memory.tags.join(", ")));
        }

        parts.join(". ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryType;

    fn test_config() -> ChunkingConfig {
        ChunkingConfig {
            enabled: true,
            max_chunk_tokens: 10,
            min_chunk_tokens: 2,
            max_tokens_hard_limit: 100,
            overlap_tokens: 2,
            paragraph_separator: "\n\n".to_string(),
            embed_metadata: true,
            metadata_weight: 0.1,
            dedupe_chunks: false,
            dedupe_chunk_threshold: 0.98,
        }
    }

    #[test]
    fn test_no_chunking_needed() {
        let config = test_config();
        let chunker = Chunker::new(config);
        let text = "Short text";

        assert!(!chunker.needs_chunking(text));
        let chunks = chunker.chunk_text(text).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].method, ChunkMethod::None);
    }

    #[test]
    fn test_paragraph_chunking() {
        let config = test_config();
        let chunker = Chunker::new(config);
        let text = "First paragraph with some words.\n\nSecond paragraph with more words.\n\nThird paragraph here.";

        let chunks = chunker.chunk_text(text).unwrap();
        assert!(chunks.len() > 1);
        assert_eq!(chunks[0].method, ChunkMethod::Paragraph);
    }

    #[test]
    fn test_sentence_chunking() {
        let config = test_config();
        let chunker = Chunker::new(config);
        // Very long paragraph that will fail paragraph chunking
        let text = "First sentence here. Second sentence with words. Third sentence also present. Fourth sentence too. Fifth sentence added.";

        let chunks = chunker.chunk_text(text).unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_metadata_text_generation() {
        let config = test_config();
        let chunker = Chunker::new(config);

        let mut memory = Memory::new(
            MemoryType::Semantic,
            "test content".to_string(),
            "work".to_string(),
        );
        memory.tags = vec!["important".to_string(), "project".to_string()];

        let metadata_text = chunker.generate_metadata_text(&memory);
        assert!(metadata_text.contains("Semantic"));
        assert!(metadata_text.contains("work"));
        assert!(metadata_text.contains("important"));
    }
}
