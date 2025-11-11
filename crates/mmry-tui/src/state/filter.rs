use mmry_core::memory::MemoryType;

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    All,
    Category(String),
    Tag(String),
    MemoryType(MemoryType),
    Recent,
    Important,
}

#[derive(Debug, Clone)]
pub struct FilterState {
    pub active_filters: Vec<Filter>,
}

impl FilterState {
    pub fn new() -> Self {
        Self {
            active_filters: vec![Filter::All],
        }
    }

    pub fn add_filter(&mut self, filter: Filter) {
        if filter == Filter::All {
            self.active_filters.clear();
            self.active_filters.push(Filter::All);
        } else {
            self.active_filters.retain(|f| *f != Filter::All);
            if !self.active_filters.contains(&filter) {
                self.active_filters.push(filter);
            }
        }
    }

    pub fn remove_filter(&mut self, filter: &Filter) {
        self.active_filters.retain(|f| f != filter);
        if self.active_filters.is_empty() {
            self.active_filters.push(Filter::All);
        }
    }

    pub fn clear(&mut self) {
        self.active_filters.clear();
        self.active_filters.push(Filter::All);
    }

    pub fn is_active(&self, filter: &Filter) -> bool {
        self.active_filters.contains(filter)
    }
}

impl Default for FilterState {
    fn default() -> Self {
        Self::new()
    }
}
