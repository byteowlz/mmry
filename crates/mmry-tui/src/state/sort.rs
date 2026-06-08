use mmry_core::memory::Memory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    DateNewest,
    DateOldest,
    ImportanceHigh,
    ImportanceLow,
    Category,
    Type,
}

#[derive(Debug, Clone)]
pub struct SortState {
    pub mode: SortMode,
}

impl SortState {
    pub fn new() -> Self {
        Self {
            mode: SortMode::DateNewest,
        }
    }

    pub fn set_mode(&mut self, mode: SortMode) {
        self.mode = mode;
    }

    pub fn sort_memories(&self, memories: &mut [Memory]) {
        match self.mode {
            SortMode::DateNewest => {
                memories.sort_by_key(|b| std::cmp::Reverse(b.created_at));
            }
            SortMode::DateOldest => {
                memories.sort_by_key(|a| a.created_at);
            }
            SortMode::ImportanceHigh => {
                memories.sort_by_key(|b| std::cmp::Reverse(b.importance));
            }
            SortMode::ImportanceLow => {
                memories.sort_by_key(|a| a.importance);
            }
            SortMode::Category => {
                memories.sort_by_key(|a| a.category.clone());
            }
            SortMode::Type => {
                memories.sort_by_key(|a| match a.memory_type {
                    mmry_core::memory::MemoryType::Episodic => 0,
                    mmry_core::memory::MemoryType::Semantic => 1,
                    mmry_core::memory::MemoryType::Procedural => 2,
                });
            }
        }
    }
}

impl Default for SortState {
    fn default() -> Self {
        Self::new()
    }
}
