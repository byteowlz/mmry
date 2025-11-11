use mmry_core::memory::MemoryType;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct FilterState {
    pub enabled_categories: HashSet<String>,
    pub enabled_tags: HashSet<String>,
    pub enabled_types: HashSet<MemoryType>,
    pub show_recent: bool,
    pub show_important: bool,
}

impl FilterState {
    pub fn new() -> Self {
        Self {
            enabled_categories: HashSet::new(),
            enabled_tags: HashSet::new(),
            enabled_types: HashSet::new(),
            show_recent: false,
            show_important: false,
        }
    }

    pub fn toggle_category(&mut self, category: &str) {
        if self.enabled_categories.contains(category) {
            self.enabled_categories.remove(category);
        } else {
            self.enabled_categories.insert(category.to_string());
        }
    }

    pub fn toggle_tag(&mut self, tag: &str) {
        if self.enabled_tags.contains(tag) {
            self.enabled_tags.remove(tag);
        } else {
            self.enabled_tags.insert(tag.to_string());
        }
    }

    pub fn toggle_type(&mut self, mem_type: MemoryType) {
        if self.enabled_types.contains(&mem_type) {
            self.enabled_types.remove(&mem_type);
        } else {
            self.enabled_types.insert(mem_type);
        }
    }

    pub fn is_category_enabled(&self, category: &str) -> bool {
        self.enabled_categories.is_empty() || self.enabled_categories.contains(category)
    }

    pub fn is_tag_enabled(&self, tag: &str) -> bool {
        self.enabled_tags.is_empty() || self.enabled_tags.contains(tag)
    }

    pub fn is_type_enabled(&self, mem_type: &MemoryType) -> bool {
        self.enabled_types.is_empty() || self.enabled_types.contains(mem_type)
    }

    pub fn isolate_category(&mut self, category: &str, _all_categories: &[String]) {
        self.enabled_categories.clear();
        self.enabled_categories.insert(category.to_string());
    }

    pub fn isolate_tag(&mut self, tag: &str, _all_tags: &[String]) {
        self.enabled_tags.clear();
        self.enabled_tags.insert(tag.to_string());
    }

    pub fn isolate_type(&mut self, mem_type: MemoryType) {
        self.enabled_types.clear();
        self.enabled_types.insert(mem_type);
    }

    pub fn clear(&mut self) {
        self.enabled_categories.clear();
        self.enabled_tags.clear();
        self.enabled_types.clear();
        self.show_recent = false;
        self.show_important = false;
    }

    pub fn has_active_filters(&self) -> bool {
        !self.enabled_categories.is_empty()
            || !self.enabled_tags.is_empty()
            || !self.enabled_types.is_empty()
            || self.show_recent
            || self.show_important
    }
}

impl Default for FilterState {
    fn default() -> Self {
        Self::new()
    }
}
