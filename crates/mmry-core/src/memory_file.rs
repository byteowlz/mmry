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
    /// Revision a supersede/deprecate was made against. Replay marks the
    /// memory contested when this is older than the revision it meets
    /// (concurrent edits merged from another machine).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<u32>,
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
            base_revision: None,
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
    /// 1 on add, +1 per supersede or deprecation attempt. Used for
    /// optimistic concurrency.
    pub revision: u32,
    /// Concurrent edits were merged (e.g. from two machines) and nobody has
    /// resolved them yet with a supersede or rm on the current revision.
    /// Contested memories are never injected automatically.
    #[serde(default)]
    pub contested: bool,
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
            check(&self.replay()?.entries)?;
            writeln!(file, "{}", serde_json::to_string(event)?)?;
            file.sync_data()?;
            Ok(())
        })();
        file.unlock()?;
        result
    }

    /// Append a supersede/deprecate of `event.target()` under the ledger
    /// lock. The memory must be active (contested memories included) and at
    /// `expected_revision` when given; the event records the revision it was
    /// made against as `base_revision`.
    pub fn append_edit(
        &self,
        mut event: MemoryEvent,
        expected_revision: Option<u32>,
    ) -> crate::Result<MemoryEvent> {
        let mut file = self.open_for_append()?;
        file.lock_exclusive()?;
        let result = (|| {
            let replay = self.replay()?;
            let target = event.target().to_owned();
            require_revision(&replay.entries, &target, expected_revision)?;
            event.base_revision = replay
                .entries
                .iter()
                .find(|entry| entry.memory_id == target)
                .map(|entry| entry.revision);
            writeln!(file, "{}", serde_json::to_string(&event)?)?;
            file.sync_data()?;
            Ok(event)
        })();
        file.unlock()?;
        result
    }

    /// Replay tolerating damage: malformed lines and conflicting duplicate
    /// event ids are reported as [`LedgerIssue`]s instead of failing, so one
    /// truncated line synced from another machine does not hide everything.
    pub fn replay(&self) -> crate::Result<Replay> {
        let mut issues = Vec::new();
        let mut by_id: HashMap<String, (Value, MemoryEvent)> = HashMap::new();
        let mut conflicting = HashSet::new();
        if self.path.exists() {
            let reader = BufReader::new(OpenOptions::new().read(true).open(&self.path)?);
            for (index, line) in reader.lines().enumerate() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let parsed = serde_json::from_str::<Value>(&line).and_then(|value| {
                    serde_json::from_value::<MemoryEvent>(value.clone()).map(|e| (value, e))
                });
                let (value, event) = match parsed {
                    Ok(pair) => pair,
                    Err(error) => {
                        issues.push(LedgerIssue::Malformed {
                            line: index + 1,
                            error: error.to_string(),
                        });
                        continue;
                    }
                };
                match by_id.get(&event.id) {
                    None => {
                        by_id.insert(event.id.clone(), (value, event));
                    }
                    Some((existing, _)) if *existing == value => {}
                    Some((_, existing)) => {
                        conflicting.insert(existing.target().to_owned());
                        conflicting.insert(event.target().to_owned());
                        issues.push(LedgerIssue::ConflictingEventId {
                            event_id: event.id.clone(),
                        });
                    }
                }
            }
        }
        let mut events: Vec<_> = by_id.into_values().map(|(_, event)| event).collect();
        events.sort_by(|a, b| (a.ts, &a.id).cmp(&(b.ts, &b.id)));
        let mut entries = project(events);
        for entry in &mut entries {
            if conflicting.contains(&entry.memory_id) {
                entry.contested = true;
            }
        }
        Ok(Replay { entries, issues })
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

    /// Active memories including expired ones, newest first. Damaged lines
    /// are skipped here; use [`MemoryFile::replay`] to see them.
    pub fn active_memories(&self) -> crate::Result<Vec<MemoryEntry>> {
        Ok(self.replay()?.entries)
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

/// Result of [`MemoryFile::replay`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Replay {
    /// Active memories (expired included), newest first.
    pub entries: Vec<MemoryEntry>,
    pub issues: Vec<LedgerIssue>,
}

/// Damage found while replaying a ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LedgerIssue {
    /// Unparseable line (e.g. truncated write); skipped.
    Malformed { line: usize, error: String },
    /// One event id with two different payloads; affected memories are
    /// contested.
    ConflictingEventId { event_id: String },
}

#[derive(Default)]
struct Tracked {
    entry: Option<MemoryEntry>,
    active: bool,
    revision: u32,
    /// Deprecated before its add was seen (clock skew).
    early_deprecation: bool,
    contested: bool,
}

/// Replay sorted events into the active set, detecting concurrent edits.
///
/// Every supersede/deprecate advances the revision. An edit whose
/// `base_revision` is older than the revision it meets was made without
/// seeing an earlier edit (two machines, merged later): the memory becomes
/// contested and stays visible. An edit made on the current revision
/// resolves the contest. Deprecations seen before their add (clock skew)
/// keep the memory inactive; supersedes before their add are ignored.
pub(crate) fn project(events: Vec<MemoryEvent>) -> Vec<MemoryEntry> {
    let mut memories: HashMap<String, Tracked> = HashMap::new();
    for event in events {
        let state = memories.entry(event.target().to_owned()).or_default();
        if event.event_type == MemoryEventType::MemoryAdd {
            let (Some(content), Some(memory_type)) = (event.content, event.memory_type) else {
                continue;
            };
            if state.entry.is_some() {
                // Two adds with one memory id.
                state.contested = true;
                continue;
            }
            state.revision = 1;
            state.active = !state.early_deprecation;
            state.entry = Some(MemoryEntry {
                memory_id: event.memory_id,
                content,
                memory_type,
                tags: event.tags,
                created_at: event.ts,
                updated_at: event.ts,
                revision: 1,
                contested: false,
                scope: event.scope,
                why: event.why,
                source: event.source,
                machine: event.machine,
                expires_at: event.expires_at,
                metadata: event.metadata,
                agent_ctx: event.agent_ctx,
            });
            continue;
        }
        let deactivate =
            event.event_type == MemoryEventType::MemoryDeprecate || event.content.is_none();
        let Some(entry) = state.entry.as_mut() else {
            state.early_deprecation |= deactivate;
            continue;
        };
        let concurrent = event
            .base_revision
            .is_some_and(|base| base < state.revision);
        state.revision += 1;
        state.contested = concurrent;
        if deactivate {
            // A concurrent removal of a changed memory keeps it visible (contested).
            if !(concurrent && state.active) {
                state.active = false;
            }
            continue;
        }
        if !state.active {
            if !concurrent {
                continue;
            }
            // Concurrent supersede of a removed memory: resurrect, contested.
            state.active = true;
        }
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
    }
    let mut values: Vec<_> = memories
        .into_values()
        .filter(|state| state.active)
        .filter_map(|state| {
            let mut entry = state.entry?;
            entry.revision = state.revision;
            entry.contested = state.contested;
            Some(entry)
        })
        .collect();
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

    /// Two machines start from `base`, edit independently, then git's
    /// `merge=union` keeps both sides' lines.
    struct TwoClones {
        _dir: tempfile::TempDir,
        a: MemoryFile,
        b: MemoryFile,
    }

    impl TwoClones {
        fn new(setup: impl FnOnce(&MemoryFile)) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let a = MemoryFile::new(dir.path().join("a.jsonl"));
            setup(&a);
            a.touch().unwrap();
            let b = MemoryFile::new(dir.path().join("b.jsonl"));
            fs::copy(a.path(), b.path()).unwrap();
            Self { _dir: dir, a, b }
        }

        fn merged(&self) -> MemoryFile {
            let text_a = fs::read_to_string(self.a.path()).unwrap();
            let mut merged = text_a.clone();
            for line in fs::read_to_string(self.b.path()).unwrap().lines() {
                if !text_a.lines().any(|existing| existing == line) {
                    merged.push_str(line);
                    merged.push('\n');
                }
            }
            let path = self.a.path().with_file_name("merged.jsonl");
            fs::write(&path, merged).unwrap();
            MemoryFile::new(path)
        }
    }

    fn edit(file: &MemoryFile, id: &str, text: Option<&str>) {
        let agent = AgentCtx::default();
        let event = text.map_or_else(
            || MemoryEvent::deprecate(id.into(), &agent),
            |text| MemoryEvent::supersede(id.into(), text.into(), "r".into(), &agent),
        );
        file.append_edit(event, None).unwrap();
    }

    fn contested(file: &MemoryFile) -> Vec<String> {
        let mut ids: Vec<_> = file
            .active_memories()
            .unwrap()
            .into_iter()
            .filter(|entry| entry.contested)
            .map(|entry| entry.memory_id)
            .collect();
        ids.sort();
        ids
    }

    fn only(file: &MemoryFile) -> MemoryEntry {
        let mut entries = file.active_memories().unwrap();
        assert_eq!(entries.len(), 1, "{entries:?}");
        entries.remove(0)
    }

    #[test]
    fn independent_adds_merge_without_contest() {
        let clones = TwoClones::new(|_| {});
        add(&clones.a, "from a");
        add(&clones.b, "from b");
        let merged = clones.merged();
        assert_eq!(merged.active_memories().unwrap().len(), 2);
        assert!(contested(&merged).is_empty());
        assert!(merged.replay().unwrap().issues.is_empty());
    }

    #[test]
    fn concurrent_supersedes_are_contested_until_resolved() {
        let mut id = String::new();
        let clones = TwoClones::new(|file| id = add(file, "v1").memory_id);
        edit(&clones.a, &id, Some("v2 from a"));
        std::thread::sleep(std::time::Duration::from_millis(5));
        edit(&clones.b, &id, Some("v2 from b"));
        let merged = clones.merged();
        assert_eq!(contested(&merged), vec![id.clone()]);
        let entry = only(&merged);
        assert_eq!((entry.content.as_str(), entry.revision), ("v2 from b", 3));

        // Resolving on the current revision clears the contest.
        edit(&merged, &id, Some("v3 agreed"));
        assert!(contested(&merged).is_empty());
        assert_eq!(only(&merged).content, "v3 agreed");
    }

    #[test]
    fn sequential_edits_after_sync_are_not_contested() {
        let mut id = String::new();
        let clones = TwoClones::new(|file| id = add(file, "v1").memory_id);
        edit(&clones.a, &id, Some("v2"));
        // b pulls a's edit before editing.
        fs::copy(clones.a.path(), clones.b.path()).unwrap();
        edit(&clones.b, &id, Some("v3"));
        let merged = clones.merged();
        assert!(contested(&merged).is_empty());
        assert_eq!(only(&merged).revision, 3);
    }

    #[test]
    fn removing_a_concurrently_changed_memory_keeps_it_contested() {
        let mut id = String::new();
        let clones = TwoClones::new(|file| id = add(file, "v1").memory_id);
        edit(&clones.a, &id, Some("v2 from a"));
        std::thread::sleep(std::time::Duration::from_millis(5));
        edit(&clones.b, &id, None);
        let merged = clones.merged();
        assert_eq!(contested(&merged), vec![id.clone()]);
        assert_eq!(only(&merged).content, "v2 from a");
        edit(&merged, &id, None);
        assert!(merged.active_memories().unwrap().is_empty());
    }

    #[test]
    fn superseding_a_concurrently_removed_memory_resurrects_it_contested() {
        let mut id = String::new();
        let clones = TwoClones::new(|file| id = add(file, "v1").memory_id);
        edit(&clones.a, &id, None);
        std::thread::sleep(std::time::Duration::from_millis(5));
        edit(&clones.b, &id, Some("v2 from b"));
        let merged = clones.merged();
        assert_eq!(contested(&merged), vec![id]);
        assert_eq!(only(&merged).content, "v2 from b");
    }

    #[test]
    fn conflicting_duplicate_event_id_is_reported_and_contested() {
        let clones = TwoClones::new(|_| {});
        let original = add(&clones.a, "same id, text a");
        let mut forged = original.clone();
        forged.content = Some("same id, text b".into());
        clones.b.append(&forged).unwrap();
        let merged = clones.merged();
        let replay = merged.replay().unwrap();
        assert_eq!(
            replay.issues,
            vec![LedgerIssue::ConflictingEventId {
                event_id: original.id
            }]
        );
        assert_eq!(contested(&merged), vec![original.memory_id]);
    }

    #[test]
    fn truncated_trailing_line_is_reported_not_fatal() {
        let (_dir, file) = ledger();
        add(&file, "survives");
        let mut text = fs::read_to_string(file.path()).unwrap();
        text.push_str("{\"schema_version\":1,\"id\":\"evt_tr");
        fs::write(file.path(), text).unwrap();
        let replay = file.replay().unwrap();
        assert_eq!(replay.entries.len(), 1);
        assert!(matches!(
            replay.issues.as_slice(),
            [LedgerIssue::Malformed { line: 2, .. }]
        ));
        assert!(
            file.read_events().is_err(),
            "strict reads still refuse damage"
        );
    }
}
