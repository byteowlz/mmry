//! Deterministic session-start preview: which memories a harness injects,
//! and the exact text it injects.
//!
//! Selection only depends on ledger state, the current machine and the
//! budget, so the same inputs always produce the same `rendered` bytes and
//! `selection_hash`. The rendered text uses dates, never relative ages.

use crate::MemoryEntry;
use crate::MemoryType;
use crate::repos::Scope;
use crate::repos::Source;
use crate::repos::SourcedMemory;
use chrono::DateTime;
use chrono::Utc;
use schemars::JsonSchema;
use serde::Serialize;
use std::fmt::Write as _;
use std::path::PathBuf;

/// Version of the preview JSON contract.
pub const PREVIEW_SCHEMA_VERSION: u32 = 1;

/// Selection limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Upper bound of estimated tokens of `rendered` (4 bytes per token).
    pub max_tokens: usize,
    /// Maximum number of entries.
    pub limit: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_tokens: 1200,
            limit: 20,
        }
    }
}

/// One injected memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PreviewEntry {
    pub memory_id: String,
    pub revision: u32,
    pub scope: Scope,
    /// `general` or the repository's directory name, as shown in `rendered`.
    pub origin: String,
    pub repo_path: Option<PathBuf>,
    pub memory_type: MemoryType,
    pub content: String,
    pub why: Option<String>,
    pub source: Option<String>,
    pub machine: Option<String>,
    pub updated_at: DateTime<Utc>,
    /// Whole days since `updated_at` (informational; not part of `rendered`).
    pub age_days: i64,
    pub expires_at: Option<DateTime<Utc>>,
    /// Always false for selected entries; contested ones are listed in
    /// [`Preview::contested`] instead.
    pub contested: bool,
}

/// A contested memory that was withheld from injection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Withheld {
    pub memory_id: String,
    pub origin: String,
    pub content: String,
}

/// Output of `mmry preview --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Preview {
    pub schema_version: u32,
    /// Machine used to filter machine-scoped memories.
    pub machine: Option<String>,
    pub entries: Vec<PreviewEntry>,
    /// Exact text to inject; empty when nothing is selected.
    pub rendered: String,
    /// Stable hash of the selection (ids, revisions) and `rendered`.
    pub selection_hash: String,
    pub estimated_tokens: usize,
    /// Eligible entries left out by the budget.
    pub omitted: usize,
    /// Contested memories, never injected; resolve with supersede/rm.
    pub contested: Vec<Withheld>,
    /// Things the harness should surface (pending migration, damage).
    pub warnings: Vec<String>,
}

const HEADER: &str = "Persistent memories (mmry) for this repository and in general. Notes from earlier sessions; \
verify before relying on them. Update with `mmry supersede <id>`, remove with `mmry rm <id>`.";

/// Select and render memories of `sources` for a session starting now.
pub fn build(
    sources: &[Source],
    machine: Option<&str>,
    budget: Budget,
    now: DateTime<Utc>,
    warnings: Vec<String>,
) -> crate::Result<Preview> {
    let mut warnings = warnings;
    let mut candidates = Vec::new();
    for source in sources {
        let label = source.label.clone();
        let replay = source.file().replay()?;
        if !replay.issues.is_empty() {
            warnings.push(format!(
                "{} has {} damaged line(s); run `mmry doctor`",
                source.ledger.display(),
                replay.issues.len()
            ));
        }
        for memory in replay.entries {
            candidates.push(SourcedMemory {
                scope: source.scope,
                repo: label.clone(),
                repo_path: source.repo_path.clone(),
                memory,
            });
        }
    }
    candidates.retain(|item| !item.memory.is_expired(now));
    candidates.retain(|item| {
        item.memory
            .machine
            .as_deref()
            .is_none_or(|wanted| Some(wanted) == machine)
    });
    // Repository memories first, then general; newest first; id tie-break.
    candidates.sort_by(|a, b| {
        (
            scope_rank(a.scope),
            std::cmp::Reverse(a.memory.updated_at),
            &a.memory.memory_id,
        )
            .cmp(&(
                scope_rank(b.scope),
                std::cmp::Reverse(b.memory.updated_at),
                &b.memory.memory_id,
            ))
    });
    let (contested, eligible): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|item| item.memory.contested);

    let mut entries = Vec::new();
    let mut body = String::new();
    let mut omitted = 0;
    for item in eligible {
        let block = render_entry(&item);
        let projected = estimate_tokens(&wrap(&format!("{body}{block}")));
        if entries.len() >= budget.limit || projected > budget.max_tokens {
            omitted += 1;
            continue;
        }
        body.push_str(&block);
        entries.push(entry(item, now));
    }
    let rendered = if entries.is_empty() {
        String::new()
    } else {
        wrap(&body)
    };
    let mut fingerprint = String::new();
    for entry in &entries {
        let _ = writeln!(fingerprint, "{}@{}", entry.memory_id, entry.revision);
    }
    fingerprint.push_str(&rendered);
    Ok(Preview {
        schema_version: PREVIEW_SCHEMA_VERSION,
        machine: machine.map(str::to_owned),
        estimated_tokens: estimate_tokens(&rendered),
        selection_hash: fnv1a_hex(&fingerprint),
        entries,
        rendered,
        omitted,
        contested: contested
            .into_iter()
            .map(|item| Withheld {
                origin: origin(&item),
                memory_id: item.memory.memory_id,
                content: item.memory.content,
            })
            .collect(),
        warnings,
    })
}

/// JSON schema of [`Preview`].
pub fn schema_json() -> crate::Result<String> {
    Ok(serde_json::to_string_pretty(&schemars::schema_for!(
        Preview
    ))?)
}

const fn scope_rank(scope: Scope) -> u8 {
    match scope {
        Scope::Repo => 0,
        Scope::General => 1,
    }
}

fn origin(item: &SourcedMemory) -> String {
    match item.scope {
        Scope::General => "general".to_owned(),
        Scope::Repo => format!("repo {}", item.repo),
    }
}

fn render_entry(item: &SourcedMemory) -> String {
    let memory = &item.memory;
    let mut block = format!(
        "- [{}] {} ({}): {}\n",
        origin(item),
        memory.memory_id,
        memory.updated_at.format("%Y-%m-%d"),
        one_line(&memory.content)
    );
    if let Some(why) = &memory.why {
        let _ = writeln!(block, "  why: {}", one_line(why));
    }
    if let Some(machine) = &memory.machine {
        let _ = writeln!(block, "  machine: {machine}");
    }
    block
}

fn wrap(body: &str) -> String {
    format!("<mmry>\n{HEADER}\n{body}</mmry>\n")
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Rough token estimate: 4 bytes per token, rounded up.
pub const fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

fn entry(item: SourcedMemory, now: DateTime<Utc>) -> PreviewEntry {
    let MemoryEntry {
        memory_id,
        content,
        memory_type,
        updated_at,
        revision,
        contested,
        why,
        source,
        machine,
        expires_at,
        ..
    } = item.memory;
    PreviewEntry {
        origin: match item.scope {
            Scope::General => "general".to_owned(),
            Scope::Repo => item.repo,
        },
        scope: item.scope,
        repo_path: item.repo_path,
        memory_id,
        revision,
        memory_type,
        content,
        why,
        source,
        machine,
        age_days: (now - updated_at).num_days(),
        updated_at,
        expires_at,
        contested,
    }
}

/// FNV-1a 64-bit as hex: stable across platforms and releases.
fn fnv1a_hex(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentCtx;
    use crate::MemoryEvent;
    use crate::MemoryFile;
    use crate::store::Checkout;
    use crate::store::Store;

    struct Fixture {
        _dir: tempfile::TempDir,
        sources: Vec<Source>,
        general: MemoryFile,
        repo: MemoryFile,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("state"));
        let app = dir.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let checkout = Checkout::at(&app).unwrap();
        let repo = store.register(&checkout).unwrap().ledger();
        let sources = vec![
            Source::general(&store),
            Source::for_checkout(&store, &checkout).unwrap(),
        ];
        Fixture {
            _dir: dir,
            sources,
            general: store.general(),
            repo,
        }
    }

    fn at(day: u32) -> DateTime<Utc> {
        format!("2026-09-{day:02}T12:00:00Z").parse().unwrap()
    }

    fn note(
        file: &MemoryFile,
        id: &str,
        day: u32,
        text: &str,
        edit: impl FnOnce(&mut MemoryEvent),
    ) {
        let mut event = MemoryEvent::add(
            text.into(),
            MemoryType::Procedural,
            vec![],
            &AgentCtx::default(),
        );
        event.memory_id = format!("mem_{id}");
        event.id = format!("evt_{id}");
        event.ts = at(day);
        edit(&mut event);
        file.append(&event).unwrap();
    }

    fn seed(fx: &Fixture) {
        note(
            &fx.repo,
            "r1",
            10,
            "Run just check-all before pushing",
            |e| {
                e.why = Some("CI runs the same gate".into());
            },
        );
        note(
            &fx.repo,
            "r2",
            12,
            "Integration tests need  a real git\nbinary",
            |_| {},
        );
        note(
            &fx.general,
            "g1",
            11,
            "Headless Chrome needs --no-sandbox",
            |e| {
                e.machine = Some("m-1".into());
            },
        );
        note(&fx.general, "g2", 11, "Only true on another machine", |e| {
            e.machine = Some("m-2".into());
        });
        note(&fx.general, "g3", 9, "Old staging token", |e| {
            e.expires_at = Some(at(20));
        });
    }

    const GOLDEN: &str = "\
<mmry>
Persistent memories (mmry) for this repository and in general. Notes from earlier sessions; verify before relying on them. Update with `mmry supersede <id>`, remove with `mmry rm <id>`.
- [repo app] mem_r2 (2026-09-12): Integration tests need a real git binary
- [repo app] mem_r1 (2026-09-10): Run just check-all before pushing
  why: CI runs the same gate
- [general] mem_g1 (2026-09-11): Headless Chrome needs --no-sandbox
  machine: m-1
</mmry>
";

    #[test]
    fn golden_rendering_and_selection() {
        let fx = fixture();
        seed(&fx);
        let preview = build(&fx.sources, Some("m-1"), Budget::default(), at(28), vec![]).unwrap();
        assert_eq!(preview.rendered, GOLDEN);
        let ids: Vec<_> = preview
            .entries
            .iter()
            .map(|e| e.memory_id.as_str())
            .collect();
        assert_eq!(ids, ["mem_r2", "mem_r1", "mem_g1"]);
        assert_eq!(preview.entries[1].origin, "app");
        assert_eq!(preview.entries[1].age_days, 18);
        assert_eq!(preview.omitted, 0);
        assert_eq!(preview.estimated_tokens, estimate_tokens(GOLDEN));
        assert_eq!(preview.selection_hash.len(), 16);
    }

    #[test]
    fn selection_is_deterministic_and_hash_tracks_changes() {
        let fx = fixture();
        seed(&fx);
        let run = |now| build(&fx.sources, Some("m-1"), Budget::default(), now, vec![]).unwrap();
        let first = run(at(28));
        let later = run(at(29));
        assert_eq!(
            (&first.rendered, &first.selection_hash),
            (&later.rendered, &later.selection_hash),
            "the passage of time alone must not change the injection"
        );
        let edit = MemoryEvent::supersede(
            "mem_r1".into(),
            "Run just check-all".into(),
            "shorter".into(),
            &AgentCtx::default(),
        );
        fx.repo.append_edit(edit, None).unwrap();
        assert_ne!(run(at(28)).selection_hash, first.selection_hash);
    }

    #[test]
    fn expiry_happens_at_its_time() {
        let fx = fixture();
        seed(&fx);
        let before = build(&fx.sources, Some("m-1"), Budget::default(), at(19), vec![]).unwrap();
        assert!(before.rendered.contains("mem_g3"));
        let after = build(&fx.sources, Some("m-1"), Budget::default(), at(20), vec![]).unwrap();
        assert!(!after.rendered.contains("mem_g3"));
    }

    #[test]
    fn budget_is_respected() {
        let fx = fixture();
        seed(&fx);
        for max_tokens in [0, 40, 60, 80, 120, 10_000] {
            let budget = Budget {
                max_tokens,
                limit: 20,
            };
            let preview = build(&fx.sources, Some("m-1"), budget, at(28), vec![]).unwrap();
            assert!(
                preview.estimated_tokens <= max_tokens,
                "{max_tokens}: {preview:?}"
            );
            assert_eq!(preview.entries.len() + preview.omitted, 3);
        }
        let limited = build(
            &fx.sources,
            Some("m-1"),
            Budget {
                max_tokens: 10_000,
                limit: 1,
            },
            at(28),
            vec![],
        )
        .unwrap();
        assert_eq!((limited.entries.len(), limited.omitted), (1, 2));
        assert_eq!(limited.entries[0].memory_id, "mem_r2");
    }

    #[test]
    fn contested_and_foreign_machine_memories_are_withheld() {
        let fx = fixture();
        seed(&fx);
        let mut concurrent = MemoryEvent::supersede(
            "mem_r1".into(),
            "Run just check-all twice".into(),
            "r".into(),
            &AgentCtx::default(),
        );
        concurrent.ts = at(13);
        concurrent.base_revision = Some(1);
        fx.repo.append(&concurrent).unwrap();
        let mut second = concurrent;
        second.id = "evt_second".into();
        second.content = Some("Run just check-all once".into());
        second.ts = at(14);
        fx.repo.append(&second).unwrap();

        let preview = build(&fx.sources, None, Budget::default(), at(28), vec![]).unwrap();
        let ids: Vec<_> = preview
            .entries
            .iter()
            .map(|e| e.memory_id.as_str())
            .collect();
        assert_eq!(
            ids,
            ["mem_r2"],
            "no machine: machine-scoped memories are withheld too"
        );
        assert_eq!(
            preview.contested,
            vec![Withheld {
                memory_id: "mem_r1".into(),
                origin: "repo app".into(),
                content: "Run just check-all once".into(),
            }]
        );
    }

    #[test]
    fn nothing_selected_renders_nothing() {
        let fx = fixture();
        let preview = build(
            &fx.sources,
            None,
            Budget::default(),
            at(28),
            vec!["w".into()],
        )
        .unwrap();
        assert_eq!(
            (preview.rendered.as_str(), preview.estimated_tokens),
            ("", 0)
        );
        assert_eq!(preview.warnings, ["w"]);
    }

    #[test]
    fn example_schemas_are_current() {
        let examples = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
        for (file, schema) in [
            ("preview.schema.json", schema_json().unwrap()),
            (
                "memory.schema.json",
                crate::repos::entry_schema_json().unwrap(),
            ),
        ] {
            let current = std::fs::read_to_string(format!("{examples}/{file}")).unwrap();
            assert_eq!(
                current.trim_end(),
                schema,
                "run `just generate-config` ({file})"
            );
        }
    }
}
