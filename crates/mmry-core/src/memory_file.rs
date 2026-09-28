//! Append-only memory ledger: one JSONL file of immutable events, replayed into
//! the active set of memories.

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
    /// `general` or `repo:<stable-id>`; set on add.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Why the memory matters / how to apply it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Where the observation came from (command, issue, URL, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Machine label for machine-only observations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    /// Reason for a supersede or deprecation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub agent_ctx: Value,
}

impl MemoryEvent {
    fn base(
        event_type: MemoryEventType,
        memory_id: String,
        target_memory_id: Option<String>,
        agent: &AgentCtx,
    ) -> Self {
        Self {
            schema_version: 1,
            id: format!("evt_{}", Uuid::new_v4()),
            ts: Utc::now(),
            event_type,
            memory_id,
            target_memory_id,
            content: None,
            memory_type: None,
            tags: Vec::new(),
            scope: None,
            why: None,
            source: None,
            machine: None,
            reason: None,
            expires_at: None,
            metadata: Value::Object(Map::default()),
            agent_ctx: agent.as_json(),
        }
    }

    pub fn add(
        content: String,
        memory_type: MemoryType,
        tags: Vec<String>,
        agent: &AgentCtx,
    ) -> Self {
        Self {
            content: Some(content),
            memory_type: Some(memory_type),
            tags,
            ..Self::base(
                MemoryEventType::MemoryAdd,
                format!("mem_{}", Uuid::new_v4()),
                None,
                agent,
            )
        }
    }

    pub fn deprecate(memory_id: String, agent: &AgentCtx) -> Self {
        Self::base(
            MemoryEventType::MemoryDeprecate,
            memory_id.clone(),
            Some(memory_id),
            agent,
        )
    }

    /// Replace the content of an active memory in place; its id is kept and
    /// its revision increases by one.
    pub fn supersede(memory_id: String, content: String, reason: String, agent: &AgentCtx) -> Self {
        Self {
            content: Some(content),
            reason: Some(reason),
            ..Self::base(
                MemoryEventType::MemorySupersede,
                memory_id.clone(),
                Some(memory_id),
                agent,
            )
        }
    }

    fn target(&self) -> &str {
        self.target_memory_id.as_deref().unwrap_or(&self.memory_id)
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
    /// 1 on add, +1 per supersede. Used for optimistic concurrency.
    pub revision: u32,
    /// Scope recorded on the add event (`general` / `repo:<stable-id>`).
    /// Named apart from the source `scope` label it is flattened next to.
    #[serde(rename = "recorded_scope", skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub metadata: Value,
    pub agent_ctx: Value,
}

impl MemoryEntry {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|at| at <= now)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredMemory {
    pub memory: MemoryEntry,
    pub score: usize,
}

pub struct MemoryFile {
    path: PathBuf,
}

impl MemoryFile {
    /// A ledger stored at an arbitrary path (e.g. inside the central state root).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The repo-local ledger `<root>/.mmry/mmry.jsonl`.
    pub fn open_at(root: impl AsRef<Path>) -> Self {
        Self::new(root.as_ref().join(MMRY_DIR).join(MEMORY_FILE))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    fn open_for_append(&self) -> crate::Result<fs::File> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?)
    }

    /// Create the ledger file if missing.
    pub fn touch(&self) -> crate::Result<()> {
        self.open_for_append().map(drop)
    }

    pub fn append(&self, event: &MemoryEvent) -> crate::Result<()> {
        self.append_checked(event, |_| Ok(()))
    }

    /// Append `event` while holding the ledger lock, after `check` accepted the
    /// current active set. Used for revision checks on supersede/deprecate.
    pub fn append_checked(
        &self,
        event: &MemoryEvent,
        check: impl FnOnce(&[MemoryEntry]) -> crate::Result<()>,
    ) -> crate::Result<()> {
        let mut file = self.open_for_append()?;
        file.lock_exclusive()?;
        let result = (|| {
            check(&project(self.read_events()?))?;
            writeln!(file, "{}", serde_json::to_string(event)?)?;
            file.sync_data()?;
            Ok(())
        })();
        file.unlock()?;
        result
    }

    /// Append raw, already-validated JSONL lines (used by migration to keep
    /// unknown fields byte-for-byte).
    pub(crate) fn append_raw_lines(&self, lines: &[String]) -> crate::Result<()> {
        let mut file = self.open_for_append()?;
        file.lock_exclusive()?;
        let result = (|| {
            for line in lines {
                writeln!(file, "{line}")?;
            }
            file.sync_data()?;
            Ok(())
        })();
        file.unlock()?;
        result
    }

    /// Non-empty lines with their parsed events, in file order.
    pub(crate) fn read_lines(&self) -> crate::Result<Vec<(String, MemoryEvent)>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let reader = BufReader::new(OpenOptions::new().read(true).open(&self.path)?);
        let mut lines = Vec::new();
        for (index, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event = serde_json::from_str(&line).map_err(|error| {
                crate::Error::InvalidInput(format!(
                    "{}:{}: malformed JSONL: {error}",
                    self.path.display(),
                    index + 1
                ))
            })?;
            lines.push((line, event));
        }
        Ok(lines)
    }

    /// All events, sorted by `(ts, id)` so replay is independent of line order.
    pub fn read_events(&self) -> crate::Result<Vec<MemoryEvent>> {
        let mut events: Vec<_> = self.read_lines()?.into_iter().map(|(_, e)| e).collect();
        events.sort_by(|a, b| (a.ts, &a.id).cmp(&(b.ts, &b.id)));
        Ok(events)
    }

    /// Active memories including expired ones, newest first.
    pub fn active_memories(&self) -> crate::Result<Vec<MemoryEntry>> {
        Ok(project(self.read_events()?))
    }

    /// Active, non-expired memories, newest first.
    pub fn current_memories(&self, now: DateTime<Utc>) -> crate::Result<Vec<MemoryEntry>> {
        let mut memories = self.active_memories()?;
        memories.retain(|memory| !memory.is_expired(now));
        Ok(memories)
    }

    pub fn search(&self, query: &str, limit: usize) -> crate::Result<Vec<ScoredMemory>> {
        let terms: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let phrase = query.to_lowercase();
        let mut hits: Vec<_> = self
            .current_memories(Utc::now())?
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

/// Replay sorted events into the active set.
///
/// Deprecations and supersedes seen before their add (clock skew between
/// devices) keep the memory inactive or are ignored respectively; conflict
/// reporting is handled separately.
pub(crate) fn project(events: Vec<MemoryEvent>) -> Vec<MemoryEntry> {
    let mut active: HashMap<String, MemoryEntry> = HashMap::new();
    let mut inactive = HashSet::new();
    for event in events {
        match event.event_type {
            MemoryEventType::MemoryAdd => {
                if inactive.contains(&event.memory_id) {
                    continue;
                }
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
                            revision: 1,
                            scope: event.scope,
                            why: event.why,
                            source: event.source,
                            machine: event.machine,
                            expires_at: event.expires_at,
                            metadata: event.metadata,
                            agent_ctx: event.agent_ctx,
                        },
                    );
                }
            }
            MemoryEventType::MemorySupersede if event.content.is_some() => {
                if let Some(entry) = active.get_mut(event.target()) {
                    if let Some(content) = event.content {
                        entry.content = content;
                    }
                    if let Some(memory_type) = event.memory_type {
                        entry.memory_type = memory_type;
                    }
                    if !event.tags.is_empty() {
                        entry.tags = event.tags;
                    }
                    if event.expires_at.is_some() {
                        entry.expires_at = event.expires_at;
                    }
                    entry.updated_at = event.ts;
                    entry.revision += 1;
                }
            }
            MemoryEventType::MemoryDeprecate | MemoryEventType::MemorySupersede => {
                let id = event.target().to_owned();
                active.remove(&id);
                inactive.insert(id);
            }
        }
    }
    let mut values: Vec<_> = active.into_values().collect();
    values.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.memory_id.cmp(&b.memory_id))
    });
    values
}

/// Parse an expiry: RFC 3339 timestamp or a duration like `12h`, `30d`, `8w`.
pub fn parse_expiry(text: &str, now: DateTime<Utc>) -> crate::Result<DateTime<Utc>> {
    if let Ok(at) = DateTime::parse_from_rfc3339(text) {
        return Ok(at.with_timezone(&Utc));
    }
    let invalid = || {
        crate::Error::InvalidInput(format!(
            "invalid expiry '{text}': use RFC 3339 (2026-12-31T00:00:00Z) or <n>h|d|w (e.g. 30d)"
        ))
    };
    let split = text.len().checked_sub(1).ok_or_else(invalid)?;
    let (number, unit) = text.split_at(split);
    let amount: i64 = number.parse().map_err(|_| invalid())?;
    let delta = match unit {
        "h" => chrono::Duration::try_hours(amount),
        "d" => chrono::Duration::try_days(amount),
        "w" => chrono::Duration::try_weeks(amount),
        _ => None,
    }
    .filter(|delta| *delta > chrono::Duration::zero())
    .ok_or_else(invalid)?;
    now.checked_add_signed(delta).ok_or_else(invalid)
}

/// Fail unless `memory_id` is active and, when given, at `expected_revision`.
pub fn require_revision(
    active: &[MemoryEntry],
    memory_id: &str,
    expected_revision: Option<u32>,
) -> crate::Result<()> {
    let entry = active
        .iter()
        .find(|entry| entry.memory_id == memory_id)
        .ok_or_else(|| crate::Error::NotFound(memory_id.to_owned()))?;
    match expected_revision {
        Some(expected) if expected != entry.revision => Err(crate::Error::InvalidInput(format!(
            "{memory_id} is at revision {}, expected {expected}; re-read it and retry",
            entry.revision
        ))),
        _ => Ok(()),
    }
}

pub fn find_workspace_root(start: &Path) -> PathBuf {
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

    fn add(file: &MemoryFile, text: &str) -> MemoryEvent {
        let event = MemoryEvent::add(
            text.into(),
            MemoryType::Procedural,
            vec!["rust".into()],
            &AgentCtx::default(),
        );
        file.append(&event).unwrap();
        event
    }

    fn ledger() -> (tempfile::TempDir, MemoryFile) {
        let dir = tempfile::tempdir().unwrap();
        let file = MemoryFile::new(dir.path().join("mmry.jsonl"));
        (dir, file)
    }

    #[test]
    fn append_replay_search_and_deprecate() {
        let (_dir, file) = ledger();
        let event = add(&file, "Run just fmt");
        assert_eq!(file.search("rust fmt", 10).unwrap().len(), 1);
        file.append(&MemoryEvent::deprecate(
            event.memory_id,
            &AgentCtx::default(),
        ))
        .unwrap();
        assert!(file.active_memories().unwrap().is_empty());
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
        let (_dir, file) = ledger();
        fs::write(file.path(), "not json\n").unwrap();
        assert!(
            file.read_events()
                .unwrap_err()
                .to_string()
                .contains("malformed JSONL")
        );
    }

    #[test]
    fn concurrent_appends_preserve_every_event() {
        let (_dir, file) = ledger();
        let path = file.path().to_path_buf();
        let threads: Vec<_> = (0..16)
            .map(|index| {
                let path = path.clone();
                std::thread::spawn(move || add(&MemoryFile::new(path), &format!("memory {index}")))
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(file.active_memories().unwrap().len(), 16);
    }

    #[test]
    fn supersede_keeps_id_and_bumps_revision() {
        let (_dir, file) = ledger();
        let event = add(&file, "use flag -x");
        let mut replacement = MemoryEvent::supersede(
            event.memory_id.clone(),
            "use flag -y".into(),
            "-x was removed".into(),
            &AgentCtx::default(),
        );
        replacement.ts = event.ts + chrono::Duration::seconds(1);
        file.append(&replacement).unwrap();
        let active = file.active_memories().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(
            (
                active[0].memory_id.as_str(),
                active[0].content.as_str(),
                active[0].revision
            ),
            (event.memory_id.as_str(), "use flag -y", 2)
        );
        assert_eq!(active[0].created_at, event.ts);
        assert_eq!(active[0].updated_at, replacement.ts);
    }

    #[test]
    fn content_less_supersede_deactivates() {
        let (_dir, file) = ledger();
        let event = add(&file, "old");
        let mut legacy = MemoryEvent::deprecate(event.memory_id, &AgentCtx::default());
        legacy.event_type = MemoryEventType::MemorySupersede;
        file.append(&legacy).unwrap();
        assert!(file.active_memories().unwrap().is_empty());
    }

    #[test]
    fn revision_check_rejects_stale_writers() {
        let (_dir, file) = ledger();
        let event = add(&file, "v1");
        let id = event.memory_id;
        let supersede = |text: &str| {
            MemoryEvent::supersede(id.clone(), text.into(), "r".into(), &AgentCtx::default())
        };
        file.append_checked(&supersede("v2"), |a| require_revision(a, &id, Some(1)))
            .unwrap();
        let error = file
            .append_checked(&supersede("v2b"), |a| require_revision(a, &id, Some(1)))
            .unwrap_err();
        assert!(
            error.to_string().contains("revision 2, expected 1"),
            "{error}"
        );
        assert_eq!(file.active_memories().unwrap()[0].content, "v2");
        let missing = file
            .append_checked(&supersede("x"), |a| {
                require_revision(a, "mem_missing", None)
            })
            .unwrap_err();
        assert!(matches!(missing, crate::Error::NotFound(_)));
    }

    #[test]
    fn replay_is_independent_of_line_order() {
        let (_dir, file) = ledger();
        let first = add(&file, "a");
        add(&file, "b");
        let mut sup = MemoryEvent::supersede(
            first.memory_id.clone(),
            "a2".into(),
            "r".into(),
            &AgentCtx::default(),
        );
        sup.ts = first.ts + chrono::Duration::seconds(5);
        file.append(&sup).unwrap();
        let expected = file.active_memories().unwrap();
        let text = fs::read_to_string(file.path()).unwrap();
        let reversed: Vec<_> = text.lines().rev().collect();
        fs::write(file.path(), reversed.join("\n") + "\n").unwrap();
        assert_eq!(file.active_memories().unwrap(), expected);
    }

    #[test]
    fn expired_memories_are_hidden_from_current_and_search() {
        let (_dir, file) = ledger();
        let now = Utc::now();
        let mut expired = MemoryEvent::add(
            "stale gotcha".into(),
            MemoryType::Semantic,
            Vec::new(),
            &AgentCtx::default(),
        );
        expired.expires_at = Some(now - chrono::Duration::hours(1));
        file.append(&expired).unwrap();
        add(&file, "fresh gotcha");
        assert_eq!(file.active_memories().unwrap().len(), 2);
        let current = file.current_memories(now).unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].content, "fresh gotcha");
        assert_eq!(file.search("gotcha", 10).unwrap().len(), 1);
    }

    #[test]
    fn expiry_parsing() {
        let now: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();
        assert_eq!(
            parse_expiry("30d", now).unwrap(),
            "2026-01-31T00:00:00Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(
            parse_expiry("2w", now).unwrap(),
            "2026-01-15T00:00:00Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(
            parse_expiry("2026-03-01T12:00:00+01:00", now).unwrap(),
            "2026-03-01T11:00:00Z".parse::<DateTime<Utc>>().unwrap()
        );
        for bad in ["", "d", "0d", "-1d", "5m", "soon"] {
            assert!(parse_expiry(bad, now).is_err(), "{bad}");
        }
    }
}
