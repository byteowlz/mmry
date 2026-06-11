//! Append-only workspace memory file.

use crate::agent_ctx::AgentCtx;
use crate::memory::MemoryType;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::fs::{self};
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use uuid::Uuid;

pub const MMRY_DIR: &str = ".mmry";
pub const MEMORY_FILE: &str = "mmry.jsonl";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryEventType {
    #[serde(rename = "memory.add", alias = "memory_add")]
    MemoryAdd,
    #[serde(rename = "memory.deprecate", alias = "memory_deprecate")]
    MemoryDeprecate,
    #[serde(rename = "memory.supersede", alias = "memory_supersede")]
    MemorySupersede,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub schema_version: u32,
    pub id: String,
    pub ts: DateTime<Utc>,
    #[serde(rename = "type")]
    pub event_type: MemoryEventType,
    pub memory_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_memory_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub agent_ctx: Value,
}

impl MemoryEvent {
    pub fn add(
        content: String,
        memory_type: MemoryType,
        tags: Vec<String>,
        agent_ctx: &AgentCtx,
    ) -> Self {
        Self {
            schema_version: 1,
            id: format!("evt_{}", Uuid::new_v4()),
            ts: Utc::now(),
            event_type: MemoryEventType::MemoryAdd,
            memory_id: format!("mem_{}", Uuid::new_v4()),
            target_memory_id: None,
            content: Some(content),
            memory_type: Some(memory_type),
            tags,
            metadata: Value::Object(Default::default()),
            agent_ctx: agent_ctx.as_json(),
        }
    }

    pub fn deprecate(memory_id: String, agent_ctx: &AgentCtx) -> Self {
        Self {
            schema_version: 1,
            id: format!("evt_{}", Uuid::new_v4()),
            ts: Utc::now(),
            event_type: MemoryEventType::MemoryDeprecate,
            memory_id: memory_id.clone(),
            target_memory_id: Some(memory_id),
            content: None,
            memory_type: None,
            tags: Vec::new(),
            metadata: Value::Object(Default::default()),
            agent_ctx: agent_ctx.as_json(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub memory_id: String,
    pub content: String,
    pub memory_type: MemoryType,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: Value,
    pub agent_ctx: Value,
}

pub struct MemoryFile {
    root: PathBuf,
}

impl MemoryFile {
    pub fn open_at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn open_current() -> crate::Result<Self> {
        Ok(Self::open_at(find_workspace_root(&std::env::current_dir()?)))
    }

    pub fn open_workspace(workspace_path: impl Into<PathBuf>) -> Self {
        Self::open_at(workspace_path)
    }

    pub fn dir(&self) -> PathBuf {
        self.root.join(MMRY_DIR)
    }

    pub fn path(&self) -> PathBuf {
        self.dir().join(MEMORY_FILE)
    }

    pub fn init(&self, tracked: bool) -> crate::Result<()> {
        fs::create_dir_all(self.dir())?;
        ensure_file(&self.path())?;
        if !tracked {
            self.ensure_gitignore()?;
        }
        Ok(())
    }

    pub fn ensure_gitignore(&self) -> crate::Result<()> {
        let gitignore_path = self.root.join(".gitignore");
        let mut existing = if gitignore_path.exists() {
            fs::read_to_string(&gitignore_path)?
        } else {
            String::new()
        };
        let mut changed = false;
        for line in [".mmry/mmry.jsonl", ".mmry/index/"] {
            if !existing
                .lines()
                .any(|existing_line| existing_line.trim() == line)
            {
                if !existing.ends_with('\n') && !existing.is_empty() {
                    existing.push('\n');
                }
                existing.push_str(line);
                existing.push('\n');
                changed = true;
            }
        }
        if changed {
            fs::write(gitignore_path, existing)?;
        }
        Ok(())
    }

    pub fn append(&self, event: &MemoryEvent) -> crate::Result<()> {
        fs::create_dir_all(self.dir())?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())?;
        let line = serde_json::to_string(event)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    }

    pub fn read_events(&self) -> crate::Result<Vec<MemoryEvent>> {
        let path = self.path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut events = read_events_from_path(&path)?;
        events.sort_by_key(|event| event.ts);
        Ok(events)
    }

    pub fn active_memories(&self) -> crate::Result<Vec<MemoryEntry>> {
        let mut memories: HashMap<String, MemoryEntry> = HashMap::new();
        let mut inactive: HashSet<String> = HashSet::new();

        for event in self.read_events()? {
            match event.event_type {
                MemoryEventType::MemoryAdd => {
                    if inactive.contains(&event.memory_id) {
                        continue;
                    }
                    if let (Some(content), Some(memory_type)) =
                        (event.content.clone(), event.memory_type.clone())
                    {
                        memories.insert(
                            event.memory_id.clone(),
                            MemoryEntry {
                                memory_id: event.memory_id,
                                content,
                                memory_type,
                                tags: event.tags,
                                created_at: event.ts,
                                updated_at: event.ts,
                                metadata: event.metadata,
                                agent_ctx: event.agent_ctx,
                            },
                        );
                    }
                }
                MemoryEventType::MemoryDeprecate | MemoryEventType::MemorySupersede => {
                    let target = event
                        .target_memory_id
                        .clone()
                        .unwrap_or_else(|| event.memory_id.clone());
                    inactive.insert(target.clone());
                    memories.remove(&target);
                }
            }
        }

        let mut values: Vec<_> = memories.into_values().collect();
        values.sort_by_key(|memory| std::cmp::Reverse(memory.updated_at));
        Ok(values)
    }

    pub fn search(&self, query: &str, limit: usize) -> crate::Result<Vec<ScoredMemory>> {
        let terms: Vec<String> = query
            .split_whitespace()
            .map(|term| term.to_lowercase())
            .filter(|term| !term.is_empty())
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let mut scored: Vec<ScoredMemory> = self
            .active_memories()?
            .into_iter()
            .filter_map(|memory| {
                let haystack =
                    format!("{} {}", memory.content, memory.tags.join(" ")).to_lowercase();
                let mut score = 0usize;
                for term in &terms {
                    score += haystack.matches(term).count() * 10;
                    if memory.tags.iter().any(|tag| tag.eq_ignore_ascii_case(term)) {
                        score += 25;
                    }
                }
                if haystack.contains(&query.to_lowercase()) {
                    score += 50;
                }
                (score > 0).then_some(ScoredMemory { memory, score })
            })
            .collect();
        scored.sort_by_key(|hit| {
            (
                std::cmp::Reverse(hit.score),
                std::cmp::Reverse(hit.memory.updated_at),
            )
        });
        scored.truncate(limit);
        Ok(scored)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredMemory {
    pub memory: MemoryEntry,
    pub score: usize,
}

/// Resolve the workspace root for a starting directory.
///
/// Prefers the nearest ancestor that already holds a `.mmry` store, so an
/// existing store is reused no matter which subdirectory a command runs from.
/// Otherwise falls back to the enclosing git repository root, so memories are
/// shared across the whole repo. With neither, the starting directory is used.
fn find_workspace_root(start: &Path) -> PathBuf {
    if let Some(dir) = start.ancestors().find(|dir| dir.join(MMRY_DIR).is_dir()) {
        return dir.to_path_buf();
    }
    if let Some(dir) = start.ancestors().find(|dir| dir.join(".git").exists()) {
        return dir.to_path_buf();
    }
    start.to_path_buf()
}

fn ensure_file(path: &Path) -> crate::Result<()> {
    if !path.exists() {
        OpenOptions::new().create_new(true).write(true).open(path)?;
    }
    Ok(())
}

fn read_events_from_path(path: &Path) -> crate::Result<Vec<MemoryEvent>> {
    let file = OpenOptions::new().read(true).open(path)?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<MemoryEvent>(&line) {
            Ok(event) => events.push(event),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "skipping invalid mmry memory file line")
            }
        }
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_project_active_memory() {
        let dir = tempfile::tempdir().unwrap();
        let memory_file = MemoryFile::open_at(dir.path());
        memory_file.init(false).unwrap();
        let event = MemoryEvent::add(
            "remember this".to_string(),
            MemoryType::Semantic,
            vec!["test".to_string()],
            &AgentCtx::default(),
        );
        let id = event.memory_id.clone();
        memory_file.append(&event).unwrap();

        let active = memory_file.active_memories().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].memory_id, id);
        assert_eq!(active[0].content, "remember this");
        let gitignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(gitignore.contains(".mmry/mmry.jsonl"));
        assert!(gitignore.contains(".mmry/index/"));
    }

    #[test]
    fn workspace_root_prefers_existing_mmry_from_subdir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(MMRY_DIR)).unwrap();
        let sub = root.join("a").join("b");
        fs::create_dir_all(&sub).unwrap();

        assert_eq!(find_workspace_root(&sub), root);
        // From inside the store dir itself, walk up to its owning workspace.
        assert_eq!(find_workspace_root(&root.join(MMRY_DIR)), root);
    }

    #[test]
    fn workspace_root_falls_back_to_git_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".git")).unwrap();
        let sub = root.join("crates").join("x");
        fs::create_dir_all(&sub).unwrap();

        assert_eq!(find_workspace_root(&sub), root);
    }

    #[test]
    fn workspace_root_defaults_to_start_without_repo_or_store() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("plain");
        fs::create_dir_all(&sub).unwrap();

        assert_eq!(find_workspace_root(&sub), sub);
    }

    #[test]
    fn search_finds_matching_memory() {
        let dir = tempfile::tempdir().unwrap();
        let memory_file = MemoryFile::open_at(dir.path());
        memory_file
            .append(&MemoryEvent::add(
                "Run just fmt after Rust edits".to_string(),
                MemoryType::Procedural,
                vec!["rust".to_string()],
                &AgentCtx::default(),
            ))
            .unwrap();
        let hits = memory_file.search("rust fmt", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].memory.content, "Run just fmt after Rust edits");
    }
}
