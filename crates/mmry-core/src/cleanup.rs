//! Reviewed cleanup: proposals first, writes only on explicit apply.
//!
//! [`propose`] is deterministic and model-free (duplicates, near-duplicates,
//! expired entries). Other proposers (a cleanup agent, a model the owner
//! chose) can emit the same [`Proposal`] JSON; [`apply`] validates every
//! proposal against the current ledger state and records it as ordinary
//! supersede/deprecate events. History is never rewritten.

use crate::AgentCtx;
use crate::MemoryEntry;
use crate::MemoryEvent;
use crate::repos::Source;
use chrono::DateTime;
use chrono::Utc;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeSet;

/// Token-set similarity at or above which two memories are near-duplicates.
pub const NEAR_DUPLICATE: f64 = 0.8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Remove `memory_id`.
    Deprecate,
    /// Replace the content of `memory_id` with `content`.
    Supersede,
}

/// One reviewable change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Proposal {
    /// Stable id derived from action, target, revision and content. May be
    /// empty in proposals from other tools.
    #[serde(default)]
    pub id: String,
    pub action: Action,
    pub memory_id: String,
    /// Revision the proposal was made against; apply refuses otherwise.
    pub revision: u32,
    /// New content for `supersede`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub reason: String,
    /// Other memories the reason refers to.
    #[serde(default)]
    pub related: Vec<String>,
    /// Current content of `memory_id`, for review.
    #[serde(default)]
    pub current: String,
    /// Who proposed it (`mmry` for built-in rules).
    #[serde(default = "default_proposer")]
    pub proposer: String,
}

fn default_proposer() -> String {
    "external".to_owned()
}

impl Proposal {
    fn new(
        action: Action,
        entry: &MemoryEntry,
        content: Option<String>,
        reason: String,
        related: Vec<String>,
    ) -> Self {
        let mut proposal = Self {
            id: String::new(),
            action,
            memory_id: entry.memory_id.clone(),
            revision: entry.revision,
            content,
            reason,
            related,
            current: entry.content.clone(),
            proposer: "mmry".to_owned(),
        };
        proposal.id = proposal.stable_id();
        proposal
    }

    /// `prop_` + hash of what the proposal changes.
    pub fn stable_id(&self) -> String {
        let key = format!(
            "{:?}\n{}\n{}\n{}",
            self.action,
            self.memory_id,
            self.revision,
            self.content.as_deref().unwrap_or("")
        );
        let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
        format!("prop_{hash:016x}")
    }
}

/// Built-in proposals for `sources`, per ledger, in a stable order.
pub fn propose(sources: &[Source], now: DateTime<Utc>) -> crate::Result<Vec<Proposal>> {
    let mut proposals = Vec::new();
    for source in sources {
        let mut entries = source.file().active_memories()?;
        // Oldest first: newer memories win duplicates.
        entries.sort_by(|a, b| (a.updated_at, &a.memory_id).cmp(&(b.updated_at, &b.memory_id)));
        let mut removed = BTreeSet::new();
        for entry in entries.iter().filter(|entry| entry.is_expired(now)) {
            removed.insert(entry.memory_id.clone());
            proposals.push(Proposal::new(
                Action::Deprecate,
                entry,
                None,
                format!(
                    "expired {}",
                    entry
                        .expires_at
                        .map(|at| at.to_rfc3339())
                        .unwrap_or_default()
                ),
                Vec::new(),
            ));
        }
        for (index, older) in entries.iter().enumerate() {
            if removed.contains(&older.memory_id) || older.contested {
                continue;
            }
            let newer = entries[index + 1..].iter().rev().find(|newer| {
                !removed.contains(&newer.memory_id)
                    && !newer.contested
                    && newer.machine == older.machine
                    && similarity(&older.content, &newer.content) >= NEAR_DUPLICATE
            });
            if let Some(newer) = newer {
                let score = similarity(&older.content, &newer.content);
                let reason = if normalize(&older.content) == normalize(&newer.content) {
                    format!("duplicate of newer {}", newer.memory_id)
                } else {
                    format!(
                        "near-duplicate of newer {} (similarity {score:.2}); review both texts",
                        newer.memory_id
                    )
                };
                removed.insert(older.memory_id.clone());
                proposals.push(Proposal::new(
                    Action::Deprecate,
                    older,
                    None,
                    reason,
                    vec![newer.memory_id.clone()],
                ));
            }
        }
    }
    Ok(proposals)
}

/// Apply one reviewed proposal to whichever of `sources` holds its memory.
///
/// Fails if the memory is gone or no longer at `proposal.revision`, or if a
/// non-empty `id` does not match the content (external proposers may leave
/// `id` empty).
pub fn apply(
    sources: &[Source],
    proposal: &Proposal,
    agent: &AgentCtx,
) -> crate::Result<MemoryEvent> {
    if !proposal.id.is_empty() && proposal.id != proposal.stable_id() {
        return Err(crate::Error::InvalidInput(format!(
            "proposal {} does not match its content (expected id {})",
            proposal.id,
            proposal.stable_id()
        )));
    }
    let reason = format!(
        "cleanup {} ({}): {}",
        proposal.id, proposal.proposer, proposal.reason
    );
    let event = match (proposal.action, &proposal.content) {
        (Action::Deprecate, _) => {
            let mut event = MemoryEvent::deprecate(proposal.memory_id.clone(), agent);
            event.reason = Some(reason);
            event
        }
        (Action::Supersede, Some(content)) if !content.trim().is_empty() => {
            MemoryEvent::supersede(proposal.memory_id.clone(), content.clone(), reason, agent)
        }
        (Action::Supersede, _) => {
            return Err(crate::Error::InvalidInput(format!(
                "proposal {} supersedes without content",
                proposal.id
            )));
        }
    };
    for source in sources {
        let file = source.file();
        if file
            .active_memories()?
            .iter()
            .any(|entry| entry.memory_id == proposal.memory_id)
        {
            return file.append_edit(event, Some(proposal.revision));
        }
    }
    Err(crate::Error::NotFound(proposal.memory_id.clone()))
}

/// JSON schema of a list of proposals (`cleanup propose --json`, `apply --file`).
pub fn schema_json() -> crate::Result<String> {
    Ok(serde_json::to_string_pretty(&schemars::schema_for!(
        Vec<Proposal>
    ))?)
}

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

fn tokens(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Jaccard similarity of lowercase token sets.
pub fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (tokens(a), tokens(b));
    let union = a.union(&b).count();
    if union == 0 {
        return 1.0;
    }
    #[expect(clippy::cast_precision_loss, reason = "token counts are small")]
    let score = a.intersection(&b).count() as f64 / union as f64;
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryFile;
    use crate::MemoryType;
    use crate::store::Store;

    fn at(day: u32) -> DateTime<Utc> {
        format!("2026-09-{day:02}T12:00:00Z").parse().unwrap()
    }

    fn note(file: &MemoryFile, id: &str, day: u32, text: &str) -> MemoryEvent {
        let mut event = MemoryEvent::add(
            text.into(),
            MemoryType::Semantic,
            Vec::new(),
            &AgentCtx::default(),
        );
        event.memory_id = format!("mem_{id}");
        event.id = format!("evt_{id}");
        event.ts = at(day);
        file.append(&event).unwrap();
        event
    }

    fn general(dir: &std::path::Path) -> (Store, Vec<Source>) {
        let store = Store::new(dir);
        let sources = vec![Source::general(&store)];
        (store, sources)
    }

    #[test]
    fn two_devices_recording_the_same_fact_get_one_proposal() {
        let dir = tempfile::tempdir().unwrap();
        let (a, _) = general(&dir.path().join("a"));
        let (b, _) = general(&dir.path().join("b"));
        note(&a.general(), "a", 1, "Staging DB needs VPN  on");
        note(&b.general(), "b", 2, "staging db needs vpn on");
        // Union-merge b's ledger into a (what git sync does).
        crate::store::merge_ledgers(&b.general(), &a.general()).unwrap();
        let sources = vec![Source::general(&a)];

        let proposals = propose(&sources, at(3)).unwrap();
        assert_eq!(proposals.len(), 1, "{proposals:?}");
        let proposal = &proposals[0];
        assert_eq!(
            (
                proposal.action,
                proposal.memory_id.as_str(),
                proposal.related.as_slice()
            ),
            (Action::Deprecate, "mem_a", ["mem_b".to_owned()].as_slice())
        );
        assert!(proposal.reason.starts_with("duplicate of newer mem_b"));
        assert_eq!(
            propose(&sources, at(3)).unwrap(),
            proposals,
            "deterministic"
        );

        apply(&sources, proposal, &AgentCtx::default()).unwrap();
        let left: Vec<_> = a
            .general()
            .active_memories()
            .unwrap()
            .into_iter()
            .map(|m| m.memory_id)
            .collect();
        assert_eq!(left, ["mem_b"]);
        assert!(propose(&sources, at(3)).unwrap().is_empty());
        assert!(
            apply(&sources, proposal, &AgentCtx::default()).is_err(),
            "not twice"
        );
    }

    #[test]
    fn near_duplicates_and_expired_entries() {
        let dir = tempfile::tempdir().unwrap();
        let (store, sources) = general(dir.path());
        let file = store.general();
        note(
            &file,
            "old",
            1,
            "always run just check-all before every push to main",
        );
        note(
            &file,
            "new",
            2,
            "Run just check-all before every push to main.",
        );
        note(&file, "other", 2, "completely unrelated memory");
        note(&file, "exp", 1, "temporary token");
        let mut edit = MemoryEvent::supersede(
            "mem_exp".into(),
            "temporary token".into(),
            "set expiry".into(),
            &AgentCtx::default(),
        );
        edit.expires_at = Some(at(2));
        file.append_edit(edit, None).unwrap();

        let proposals = propose(&sources, at(3)).unwrap();
        let summary: Vec<_> = proposals
            .iter()
            .map(|p| (p.memory_id.as_str(), p.reason.split(' ').next().unwrap()))
            .collect();
        assert_eq!(
            summary,
            [("mem_exp", "expired"), ("mem_old", "near-duplicate")]
        );
        assert!(
            similarity(
                "always run just check-all before every push to main",
                "Run just check-all before every push to main."
            ) >= NEAR_DUPLICATE
        );
        assert!(similarity("a b c", "x y z") < 0.1);
    }

    #[test]
    fn stale_or_tampered_proposals_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (store, sources) = general(dir.path());
        note(&store.general(), "x", 1, "same text");
        note(&store.general(), "y", 2, "same text");
        let proposal = propose(&sources, at(3)).unwrap().remove(0);

        let mut tampered = proposal.clone();
        tampered.memory_id = "mem_y".into();
        assert!(
            apply(&sources, &tampered, &AgentCtx::default())
                .unwrap_err()
                .to_string()
                .contains("does not match")
        );

        // The memory changed after the proposal was made.
        store
            .general()
            .append_edit(
                MemoryEvent::supersede(
                    "mem_x".into(),
                    "edited".into(),
                    "r".into(),
                    &AgentCtx::default(),
                ),
                None,
            )
            .unwrap();
        let error = apply(&sources, &proposal, &AgentCtx::default()).unwrap_err();
        assert!(error.to_string().contains("expected 1"), "{error}");
    }

    #[test]
    fn external_supersede_proposals_apply() {
        let dir = tempfile::tempdir().unwrap();
        let (store, sources) = general(dir.path());
        note(&store.general(), "x", 1, "use flag -x");
        let mut proposal = Proposal {
            id: String::new(),
            action: Action::Supersede,
            memory_id: "mem_x".into(),
            revision: 1,
            content: Some("use flag -y".into()),
            reason: "-x removed upstream".into(),
            related: Vec::new(),
            current: String::new(),
            proposer: "cleanup-agent".into(),
        };
        proposal.id = proposal.stable_id();
        apply(&sources, &proposal, &AgentCtx::default()).unwrap();
        let entry = store.general().active_memories().unwrap().remove(0);
        assert_eq!((entry.content.as_str(), entry.revision), ("use flag -y", 2));
    }

    #[test]
    fn example_schema_is_current() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/cleanup.schema.json"
        );
        let current = std::fs::read_to_string(path).unwrap();
        assert_eq!(
            current.trim_end(),
            schema_json().unwrap(),
            "run `just generate-config`"
        );
    }
}
