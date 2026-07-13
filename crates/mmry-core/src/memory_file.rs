//! Append-only workspace memory ledger.

use crate::agent_ctx::AgentCtx;
use chrono::DateTime;
use chrono::Utc;
use fs2::FileExt;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    Episodic,
    Semantic,
    Procedural,
}

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
        agent: &AgentCtx,
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
            metadata: Value::Object(Map::default()),
            agent_ctx: agent.as_json(),
        }
    }

    pub fn deprecate(memory_id: String, agent: &AgentCtx) -> Self {
        Self {
            schema_version: 1,
            id: format!("evt_{}", Uuid::new_v4()),
            ts: Utc::now(),
            event_type: MemoryEventType::MemoryDeprecate,
            target_memory_id: Some(memory_id.clone()),
            memory_id,
            content: None,
            memory_type: None,
            tags: Vec::new(),
            metadata: Value::Object(Map::default()),
            agent_ctx: agent.as_json(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredMemory {
    pub memory: MemoryEntry,
    pub score: usize,
}

pub struct MemoryFile {
    root: PathBuf,
}

impl MemoryFile {
    pub fn open_at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn open_current() -> crate::Result<Self> {
        Ok(Self::open_at(
            find_workspace_root(&std::env::current_dir()?),
        ))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn dir(&self) -> PathBuf {
        self.root.join(MMRY_DIR)
    }
    pub fn path(&self) -> PathBuf {
        self.dir().join(MEMORY_FILE)
    }

    pub fn init(&self, tracked: bool) -> crate::Result<()> {
        fs::create_dir_all(self.dir())?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())?;
        if !tracked {
            self.ensure_gitignore()?;
        }
        Ok(())
    }

    fn ensure_gitignore(&self) -> crate::Result<()> {
        let path = self.root.join(".gitignore");
        let mut text = fs::read_to_string(&path).unwrap_or_default();
        if !text.lines().any(|line| line.trim() == ".mmry/mmry.jsonl") {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(".mmry/mmry.jsonl\n");
            fs::write(path, text)?;
        }
        Ok(())
    }

    pub fn append(&self, event: &MemoryEvent) -> crate::Result<()> {
        fs::create_dir_all(self.dir())?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())?;
        file.lock_exclusive()?;
        writeln!(file, "{}", serde_json::to_string(event)?)?;
        file.sync_data()?;
        file.unlock()?;
        Ok(())
    }

    pub fn read_events(&self) -> crate::Result<Vec<MemoryEvent>> {
        if !self.path().exists() {
            return Ok(Vec::new());
        }
        let reader = BufReader::new(OpenOptions::new().read(true).open(self.path())?);
        let mut events = Vec::new();
        for (index, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event = serde_json::from_str(&line).map_err(|error| {
                crate::Error::InvalidInput(format!(
                    "{}:{}: malformed JSONL: {error}",
                    self.path().display(),
                    index + 1
                ))
            })?;
            events.push(event);
        }
        events.sort_by_key(|event: &MemoryEvent| (event.ts, event.id.clone()));
        Ok(events)
    }

    pub fn active_memories(&self) -> crate::Result<Vec<MemoryEntry>> {
        let mut active = HashMap::new();
        let mut inactive = HashSet::new();
        for event in self.read_events()? {
            match event.event_type {
                MemoryEventType::MemoryAdd if !inactive.contains(&event.memory_id) => {
                    if let (Some(content), Some(memory_type)) = (event.content, event.memory_type) {
                        active.insert(
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
                    let id = event.target_memory_id.unwrap_or(event.memory_id);
                    inactive.insert(id.clone());
                    active.remove(&id);
                }
                MemoryEventType::MemoryAdd => {}
            }
        }
        let mut values: Vec<_> = active.into_values().collect();
        values.sort_by_key(|m| (std::cmp::Reverse(m.updated_at), m.memory_id.clone()));
        Ok(values)
    }

    pub fn search(&self, query: &str, limit: usize) -> crate::Result<Vec<ScoredMemory>> {
        let terms: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let phrase = query.to_lowercase();
        let mut hits: Vec<_> = self
            .active_memories()?
            .into_iter()
            .filter_map(|memory| {
                let text = format!("{} {}", memory.content, memory.tags.join(" ")).to_lowercase();
                let score = terms
                    .iter()
                    .map(|term| text.matches(term).count() * 10)
                    .sum::<usize>()
                    + usize::from(text.contains(&phrase)) * 50;
                (score > 0).then_some(ScoredMemory { memory, score })
            })
            .collect();
        hits.sort_by_key(|hit| {
            (
                std::cmp::Reverse(hit.score),
                std::cmp::Reverse(hit.memory.updated_at),
                hit.memory.memory_id.clone(),
            )
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

fn find_workspace_root(start: &Path) -> PathBuf {
    start
        .ancestors()
        .find(|dir| dir.join(MMRY_DIR).is_dir())
        .or_else(|| start.ancestors().find(|dir| dir.join(".git").exists()))
        .unwrap_or(start)
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn append_replay_search_and_deprecate() {
        let dir = tempfile::tempdir().unwrap();
        let file = MemoryFile::open_at(dir.path());
        file.init(false).unwrap();
        let event = MemoryEvent::add(
            "Run just fmt".into(),
            MemoryType::Procedural,
            vec!["rust".into()],
            &AgentCtx::default(),
        );
        file.append(&event).unwrap();
        assert_eq!(file.search("rust fmt", 10).unwrap().len(), 1);
        file.append(&MemoryEvent::deprecate(
            event.memory_id,
            &AgentCtx::default(),
        ))
        .unwrap();
        assert!(file.active_memories().unwrap().is_empty());
        assert!(!fs::read_to_string(dir.path().join(".gitignore"))
            .unwrap()
            .contains("index"));
    }

    #[test]
    fn legacy_event_spelling_remains_replayable() {
        let event = MemoryEvent::add(
            "compatible".into(),
            MemoryType::Semantic,
            Vec::new(),
            &AgentCtx::default(),
        );
        let json = serde_json::to_string(&event)
            .unwrap()
            .replace("memory.add", "memory_add");
        assert_eq!(
            serde_json::from_str::<MemoryEvent>(&json)
                .unwrap()
                .event_type,
            MemoryEventType::MemoryAdd
        );
    }

    #[test]
    fn malformed_lines_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = MemoryFile::open_at(dir.path());
        file.init(true).unwrap();
        fs::write(file.path(), "not json\n").unwrap();
        assert!(file
            .read_events()
            .unwrap_err()
            .to_string()
            .contains("malformed JSONL"));
    }

    #[test]
    fn tracked_init_does_not_create_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        MemoryFile::open_at(dir.path()).init(true).unwrap();
        assert!(!dir.path().join(".gitignore").exists());
    }

    #[test]
    fn concurrent_appends_preserve_every_event() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let threads: Vec<_> = (0..16)
            .map(|index| {
                let root = root.clone();
                std::thread::spawn(move || {
                    MemoryFile::open_at(root)
                        .append(&MemoryEvent::add(
                            format!("memory {index}"),
                            MemoryType::Semantic,
                            Vec::new(),
                            &AgentCtx::default(),
                        ))
                        .unwrap();
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(
            MemoryFile::open_at(root).active_memories().unwrap().len(),
            16
        );
    }
}
