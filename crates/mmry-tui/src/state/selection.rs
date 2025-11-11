use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct Selection {
    pub index: usize,
    pub offset: usize,
    pub selected_indices: HashSet<usize>,
}

impl Selection {
    pub fn new() -> Self {
        Self {
            index: 0,
            offset: 0,
            selected_indices: HashSet::new(),
        }
    }

    pub fn toggle_selection(&mut self) {
        if self.selected_indices.contains(&self.index) {
            self.selected_indices.remove(&self.index);
        } else {
            self.selected_indices.insert(self.index);
        }
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected_indices.contains(&index)
    }

    pub fn select_all(&mut self, max: usize) {
        self.selected_indices.clear();
        for i in 0..max {
            self.selected_indices.insert(i);
        }
    }

    pub fn deselect_all(&mut self) {
        self.selected_indices.clear();
    }

    pub fn has_selections(&self) -> bool {
        !self.selected_indices.is_empty()
    }

    pub fn selection_count(&self) -> usize {
        self.selected_indices.len()
    }

    pub fn get_selected_indices(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = self.selected_indices.iter().copied().collect();
        indices.sort();
        indices
    }

    pub fn next(&mut self, max: usize, page_size: usize) {
        if self.index < max.saturating_sub(1) {
            self.index += 1;
            
            if self.index >= self.offset + page_size {
                self.offset = self.index.saturating_sub(page_size - 1);
            }
        }
    }

    pub fn previous(&mut self) {
        if self.index > 0 {
            self.index -= 1;
            
            if self.index < self.offset {
                self.offset = self.index;
            }
        }
    }

    pub fn top(&mut self) {
        self.index = 0;
        self.offset = 0;
    }

    pub fn bottom(&mut self, max: usize, page_size: usize) {
        if max > 0 {
            self.index = max - 1;
            self.offset = max.saturating_sub(page_size);
        }
    }

    pub fn page_down(&mut self, max: usize, page_size: usize) {
        self.index = (self.index + page_size).min(max.saturating_sub(1));
        self.offset = (self.offset + page_size).min(max.saturating_sub(page_size));
    }

    pub fn page_up(&mut self, page_size: usize) {
        self.index = self.index.saturating_sub(page_size);
        self.offset = self.offset.saturating_sub(page_size);
    }

    pub fn reset(&mut self) {
        self.index = 0;
        self.offset = 0;
        self.selected_indices.clear();
    }

    pub fn remove_indices(&mut self, removed_indices: &[usize]) {
        for &idx in removed_indices {
            self.selected_indices.remove(&idx);
        }
        
        let mut new_selected = HashSet::new();
        for &selected_idx in &self.selected_indices {
            let removed_before = removed_indices.iter().filter(|&&i| i < selected_idx).count();
            new_selected.insert(selected_idx - removed_before);
        }
        self.selected_indices = new_selected;
    }
}

impl Default for Selection {
    fn default() -> Self {
        Self::new()
    }
}
