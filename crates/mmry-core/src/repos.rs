//! Ledger sources (general, central repos, tracked repo-local ledgers) and
//! bounded, in-memory aggregation across them.

use crate::MemoryEntry;
use crate::MemoryFile;
use crate::ScoredMemory;
use crate::config::DiscoveryRoot;
use crate::store::Checkout;
use crate::store::Store;
use chrono::Utc;
use rayon::prelude::*;
use serde::Serialize;
use std::path::PathBuf;
use walkdir::DirEntry;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    General,
    Repo,
}

/// Where a ledger lives.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    Central,
    Tracked,
}

/// JSON schema of the entry objects printed by `list`, `search`, `add`,
/// `supersede` and `rm` (`search` adds `score`, `rm` adds `removed`).
pub fn entry_schema_json() -> crate::Result<String> {
    Ok(serde_json::to_string_pretty(&schemars::schema_for!(
        SourcedMemory
    ))?)
}

/// One ledger that can be read.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Source {
    pub scope: Scope,
    /// `general`, or the repository's store-unique name (`<name>--<id>` in
    /// the central store).
    pub name: String,
    /// Human label: `general` or the repository's directory name.
    pub label: String,
    pub repo_path: Option<PathBuf>,
    pub storage: Storage,
    pub ledger: PathBuf,
    pub exists: bool,
}

impl Source {
    pub fn general(store: &Store) -> Self {
        Self::new(
            Scope::General,
            "general".into(),
            None,
            Storage::Central,
            &store.general(),
        )
    }

    fn new(
        scope: Scope,
        name: String,
        repo_path: Option<PathBuf>,
        storage: Storage,
        ledger: &MemoryFile,
    ) -> Self {
        Self {
            scope,
            label: name.clone(),
            name,
            repo_path,
            storage,
            exists: ledger.exists(),
            ledger: ledger.path().to_path_buf(),
        }
    }

    /// Central or tracked source for `checkout`; central lookups do not write.
    pub fn for_checkout(store: &Store, checkout: &Checkout) -> crate::Result<Self> {
        if checkout.tracked().is_some() {
            return Ok(Self::new(
                Scope::Repo,
                checkout.name.clone(),
                Some(checkout.root.clone()),
                Storage::Tracked,
                &checkout.local_ledger(),
            ));
        }
        let repo = store.plan(checkout)?;
        Ok(Self {
            label: repo.record.name.clone(),
            ..Self::new(
                Scope::Repo,
                repo.dir_name(),
                Some(checkout.root.clone()),
                Storage::Central,
                &repo.ledger(),
            )
        })
    }

    pub fn file(&self) -> MemoryFile {
        MemoryFile::new(&self.ledger)
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SourcedMemory {
    pub scope: Scope,
    pub repo: String,
    pub repo_path: Option<PathBuf>,
    #[serde(flatten)]
    pub memory: MemoryEntry,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourcedHit {
    pub scope: Scope,
    pub repo: String,
    pub repo_path: Option<PathBuf>,
    pub score: usize,
    #[serde(flatten)]
    pub memory: MemoryEntry,
}

/// General + every central repo + tracked repo-local ledgers under `roots`.
pub fn all_sources(store: &Store, roots: &[DiscoveryRoot]) -> crate::Result<Vec<Source>> {
    let mut sources = vec![Source::general(store)];
    for repo in store.repos()? {
        sources.push(Source {
            label: repo.record.name.clone(),
            ..Source::new(
                Scope::Repo,
                repo.dir_name(),
                repo.checkouts.first().cloned(),
                Storage::Central,
                &repo.ledger(),
            )
        });
    }
    sources.extend(
        discover_local(roots)?
            .into_iter()
            .filter(|checkout| checkout.tracked().is_some())
            .map(|checkout| {
                Source::new(
                    Scope::Repo,
                    checkout.name.clone(),
                    Some(checkout.root.clone()),
                    Storage::Tracked,
                    &checkout.local_ledger(),
                )
            }),
    );
    Ok(sources)
}

/// Checkouts under `roots` that contain a repo-local `.mmry/mmry.jsonl`.
pub fn discover_local(roots: &[DiscoveryRoot]) -> crate::Result<Vec<Checkout>> {
    let mut found = Vec::new();
    for root in roots {
        let canonical_root = std::fs::canonicalize(&root.path).map_err(|error| {
            crate::Error::Config(format!(
                "cannot access discovery root {}: {error}",
                root.path.display()
            ))
        })?;
        for entry in WalkDir::new(&canonical_root)
            .max_depth(root.max_depth)
            .follow_links(false)
            .into_iter()
            .filter_entry(include_entry)
        {
            let entry = entry.map_err(|error| crate::Error::Other(error.into()))?;
            if entry.file_type().is_dir() && entry.path().join(".mmry/mmry.jsonl").exists() {
                found.push(Checkout::at(entry.path())?);
            }
        }
    }
    found.sort_by(|a, b| a.root.cmp(&b.root));
    found.dedup_by(|a, b| a.root == b.root);
    Ok(found)
}

/// Result of a best-effort scan for repo-local ledgers.
#[derive(Debug, Default, Clone)]
pub struct Scan {
    pub found: Vec<Checkout>,
    /// Directories that could not be read (permissions, vanished); skipped.
    pub unreadable: Vec<PathBuf>,
}

/// Best-effort search for `.mmry/mmry.jsonl` below `roots`.
///
/// Like [`discover_local`] but tolerant: unreadable directories are recorded instead of aborting, and
/// anything under `exclude` (e.g. the central store) is skipped.
pub fn scan_local(roots: &[DiscoveryRoot], exclude: &[PathBuf]) -> Scan {
    let mut scan = Scan::default();
    for root in roots {
        let Ok(canonical_root) = std::fs::canonicalize(&root.path) else {
            scan.unreadable.push(root.path.clone());
            continue;
        };
        let walker = WalkDir::new(&canonical_root)
            .max_depth(root.max_depth)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                include_entry(entry) && !exclude.iter().any(|path| entry.path().starts_with(path))
            });
        for entry in walker {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_dir() && entry.path().join(".mmry/mmry.jsonl").exists()
                    {
                        match Checkout::at(entry.path()) {
                            Ok(checkout) => scan.found.push(checkout),
                            Err(_) => scan.unreadable.push(entry.path().to_path_buf()),
                        }
                    }
                }
                Err(error) => {
                    if let Some(path) = error.path() {
                        scan.unreadable.push(path.to_path_buf());
                    }
                }
            }
        }
    }
    scan.found.sort_by(|a, b| a.root.cmp(&b.root));
    scan.found.dedup_by(|a, b| a.root == b.root);
    scan.unreadable.sort();
    scan.unreadable.dedup();
    scan
}

fn include_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !matches!(
        name.as_ref(),
        ".git"
            | "target"
            | "node_modules"
            | ".cache"
            | ".cargo"
            | ".rustup"
            | ".npm"
            | ".pnpm-store"
            | ".bun"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".local"
            | ".Trash"
            | "Library"
            | "snap"
    )
}

/// The single repository source called `name` (general excluded).
pub fn select_named(sources: &[Source], name: &str) -> crate::Result<Source> {
    let matches: Vec<_> = sources
        .iter()
        .filter(|source| source.scope == Scope::Repo)
        .filter(|source| {
            source.name == name
                || source
                    .name
                    .rsplit_once("--")
                    .is_some_and(|(readable, _)| readable == name)
                || source
                    .repo_path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .is_some_and(|file| file == name)
        })
        .cloned()
        .collect();
    match matches.as_slice() {
        [] => Err(crate::Error::NotFound(format!("repository '{name}'"))),
        [source] => Ok(source.clone()),
        _ => Err(crate::Error::InvalidInput(format!(
            "repository name '{name}' is ambiguous; use one of the store names:\n{}",
            matches
                .iter()
                .map(|source| {
                    format!(
                        "  {}\t{}",
                        source.name,
                        source
                            .repo_path
                            .as_ref()
                            .map_or_else(String::new, |p| p.display().to_string())
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        ))),
    }
}

fn sort_key(scope: Scope, source: &Source) -> (Scope, String) {
    (scope, source.ledger.display().to_string())
}

/// Active, non-expired memories from `sources`, newest first.
pub fn list(sources: &[Source]) -> crate::Result<Vec<SourcedMemory>> {
    let now = Utc::now();
    collect(sources, |file| file.current_memories(now))
}

/// Active memories from `sources` including expired ones, newest first.
pub fn list_including_expired(sources: &[Source]) -> crate::Result<Vec<SourcedMemory>> {
    collect(sources, MemoryFile::active_memories)
}

fn collect(
    sources: &[Source],
    read: impl Fn(&MemoryFile) -> crate::Result<Vec<MemoryEntry>> + Sync,
) -> crate::Result<Vec<SourcedMemory>> {
    let results: Vec<crate::Result<Vec<MemoryEntry>>> = sources
        .par_iter()
        .map(|source| read(&source.file()))
        .collect();
    let mut merged = Vec::new();
    for (source, memories) in sources.iter().zip(results) {
        for memory in memories? {
            merged.push((
                sort_key(source.scope, source),
                SourcedMemory {
                    scope: source.scope,
                    repo: source.name.clone(),
                    repo_path: source.repo_path.clone(),
                    memory,
                },
            ));
        }
    }
    merged.sort_by(|(ka, a), (kb, b)| {
        b.memory
            .updated_at
            .cmp(&a.memory.updated_at)
            .then_with(|| ka.cmp(kb))
            .then_with(|| a.memory.memory_id.cmp(&b.memory.memory_id))
    });
    Ok(merged.into_iter().map(|(_, item)| item).collect())
}

pub fn search(sources: &[Source], query: &str, limit: usize) -> crate::Result<Vec<SourcedHit>> {
    let results: Vec<crate::Result<Vec<ScoredMemory>>> = sources
        .par_iter()
        .map(|source| source.file().search(query, usize::MAX))
        .collect();
    let mut merged = Vec::new();
    for (source, hits) in sources.iter().zip(results) {
        for hit in hits? {
            merged.push((
                sort_key(source.scope, source),
                SourcedHit {
                    scope: source.scope,
                    repo: source.name.clone(),
                    repo_path: source.repo_path.clone(),
                    score: hit.score,
                    memory: hit.memory,
                },
            ));
        }
    }
    merged.sort_by(|(ka, a), (kb, b)| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.memory.updated_at.cmp(&a.memory.updated_at))
            .then_with(|| ka.cmp(kb))
            .then_with(|| a.memory.memory_id.cmp(&b.memory.memory_id))
    });
    merged.truncate(limit);
    Ok(merged.into_iter().map(|(_, item)| item).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentCtx;
    use crate::MemoryEvent;
    use crate::MemoryType;
    use std::fs;
    use std::path::Path;

    fn note(file: &MemoryFile, text: &str) {
        file.append(&MemoryEvent::add(
            text.into(),
            MemoryType::Semantic,
            vec![],
            &AgentCtx::default(),
        ))
        .unwrap();
    }

    /// Repo-local ledger in tracked mode (`.mmry/tracked`).
    fn tracked_repo(root: &Path, name: &str, text: &str) -> PathBuf {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        note(&crate::store::init_tracked(&path).unwrap(), text);
        path
    }

    fn roots(path: &Path, max_depth: usize) -> Vec<DiscoveryRoot> {
        vec![DiscoveryRoot {
            path: path.into(),
            max_depth,
        }]
    }

    #[test]
    fn discovers_excludes_and_aggregates() {
        let dir = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let store = Store::new(state.path());
        tracked_repo(dir.path(), "a", "release alpha");
        tracked_repo(dir.path(), "b", "release beta");
        tracked_repo(&dir.path().join("target"), "ignored", "release");
        note(&store.general(), "release general");
        let sources = all_sources(&store, &roots(dir.path(), 2)).unwrap();
        assert_eq!(sources.len(), 3);
        let hits = search(&sources, "release", 10).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits.iter()
                .filter(|hit| hit.scope == Scope::General)
                .count(),
            1
        );
    }

    #[test]
    fn untracked_local_ledgers_are_not_read_by_all() {
        let dir = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending");
        note(&MemoryFile::open_at(&pending), "not migrated");
        assert_eq!(discover_local(&roots(dir.path(), 2)).unwrap().len(), 1);
        let sources = all_sources(&Store::new(state.path()), &roots(dir.path(), 2)).unwrap();
        assert_eq!(sources.len(), 1, "only general: {sources:?}");
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlink_loops() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        tracked_repo(dir.path(), "a", "x");
        symlink(dir.path(), dir.path().join("a/loop")).unwrap();
        assert_eq!(discover_local(&roots(dir.path(), 8)).unwrap().len(), 1);
    }

    #[test]
    fn scan_is_tolerant_and_honours_excludes() {
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("work/deep/app");
        note(&MemoryFile::open_at(&pending), "legacy");
        let excluded = dir.path().join("store");
        note(&MemoryFile::open_at(&excluded), "inside the store");
        let mut scan_roots = roots(dir.path(), 6);
        scan_roots.push(DiscoveryRoot {
            path: dir.path().join("missing"),
            max_depth: 2,
        });
        let scan = scan_local(
            &scan_roots,
            &[fs::canonicalize(dir.path()).unwrap().join("store")],
        );
        let found: Vec<_> = scan.found.iter().map(|c| c.root.clone()).collect();
        assert_eq!(found, vec![fs::canonicalize(&pending).unwrap()]);
        assert_eq!(scan.unreadable, vec![dir.path().join("missing")]);
    }

    #[test]
    fn named_selection_handles_success_zero_and_ambiguity() {
        let dir = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        tracked_repo(&dir.path().join("one"), "same", "x");
        tracked_repo(&dir.path().join("two"), "same", "y");
        tracked_repo(dir.path(), "unique", "z");
        let sources = all_sources(&Store::new(state.path()), &roots(dir.path(), 3)).unwrap();
        assert_eq!(select_named(&sources, "unique").unwrap().name, "unique");
        assert!(matches!(
            select_named(&sources, "missing"),
            Err(crate::Error::NotFound(_))
        ));
        assert!(matches!(
            select_named(&sources, "general"),
            Err(crate::Error::NotFound(_))
        ));
        let error = select_named(&sources, "same").unwrap_err().to_string();
        assert!(error.contains("one/same"));
        assert!(error.contains("two/same"));
    }

    #[test]
    fn aggregation_order_has_stable_source_tie_break() {
        let dir = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        tracked_repo(dir.path(), "b", "same");
        tracked_repo(dir.path(), "a", "same");
        let sources = all_sources(&Store::new(state.path()), &roots(dir.path(), 1)).unwrap();
        let order = |items: Vec<SourcedMemory>| {
            items
                .into_iter()
                .map(|item| item.repo_path)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            order(list(&sources).unwrap()),
            order(list(&sources).unwrap())
        );
    }

    #[test]
    fn five_hundred_repository_fixture_completes() {
        let dir = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        for index in 0..500 {
            tracked_repo(dir.path(), &format!("r{index}"), "small ledger");
        }
        let start = std::time::Instant::now();
        let sources = all_sources(&Store::new(state.path()), &roots(dir.path(), 1)).unwrap();
        let memories = list(&sources).unwrap();
        eprintln!("500 repository cold fixture: {:?}", start.elapsed());
        assert_eq!(memories.len(), 500);
    }
}
