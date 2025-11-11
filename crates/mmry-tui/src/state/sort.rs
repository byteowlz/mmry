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
                memories.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            }
            SortMode::DateOldest => {
                memories.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            }
            SortMode::ImportanceHigh => {
                memories.sort_by(|a, b| b.importance.cmp(&a.importance));
            }
            SortMode::ImportanceLow => {
                memories.sort_by(|a, b| a.importance.cmp(&b.importance));
            }
            SortMode::Category => {
                memories.sort_by(|a, b| a.category.cmp(&b.category));
            }
            SortMode::Type => {
                memories.sort_by(|a, b| {
                    let a_val = match a.memory_type {
                        mmry_core::memory::MemoryType::Episodic => 0,
                        mmry_core::memory::MemoryType::Semantic => 1,
                        mmry_core::memory::MemoryType::Procedural => 2,
                    };
                    let b_val = match b.memory_type {
                        mmry_core::memory::MemoryType::Episodic => 0,
                        mmry_core::memory::MemoryType::Semantic => 1,
                        mmry_core::memory::MemoryType::Procedural => 2,
                    };
                    a_val.cmp(&b_val)
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
