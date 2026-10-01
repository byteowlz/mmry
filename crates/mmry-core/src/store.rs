//! Where ledgers live and how repo-local ledgers move into the central store.
//!
//! Layout of the per-user store:
//!
//! ```text
//! <store_root>/general/mmry.jsonl
//! <store_root>/repos/<name>--<id>/mmry.jsonl
//! <store_root>/repos/<name>--<id>/repo.json      # {identity, name}, written once
//! <store_root>/local/checkouts.json              # this machine's checkout paths
//! ```
//!
//! Everything outside `local/` is safe to sync between machines: directory
//! names are derived from the repository identity (not registration order),
//! `repo.json` never changes, and ledgers are append-only. If two machines
//! name the same repository differently, the extra directories are merged
//! into the first one by event id on the next registration.
//!
//! A repository is in exactly one storage mode: central (default) or tracked
//! (its ledger lives in `<repo>/.mmry/` and is committed with the repo). A
//! repo is tracked when `.mmry/tracked` exists or `.mmry/mmry.jsonl` is
//! tracked by git. mmry never writes to or reads from both stores for one repo.

use crate::MemoryFile;
use crate::memory_file::MEMORY_FILE;
use crate::memory_file::MMRY_DIR;
use crate::memory_file::MemoryEvent;
use crate::memory_file::project;
use chrono::Utc;
use fs2::FileExt;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

const GENERAL_DIR: &str = "general";
const REPOS_DIR: &str = "repos";
const REGISTRY_FILE: &str = "repo.json";
/// Machine-local state that must never be synced.
pub const LOCAL_DIR: &str = "local";
const CHECKOUTS_FILE: &str = "checkouts.json";
/// Marker selecting tracked (repo-local) mode.
pub const TRACKED_MARKER: &str = "tracked";
/// Note left in `.mmry/` after migration, pointing at the central ledger.
pub const MIGRATED_MARKER: &str = "MIGRATED";

/// Registry entry binding a central repo directory to a stable identity.
///
/// Immutable once written so it never conflicts when the store is synced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRecord {
    /// `git:<root-commit>` or `path:<canonical path>`.
    pub identity: String,
    /// Readable name (directory name of the first registered checkout).
    pub name: String,
}

/// A repository directory inside the central store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CentralRepo {
    pub dir: PathBuf,
    pub record: RepoRecord,
    /// Checkout paths known on this machine (machine-local, not synced).
    pub checkouts: Vec<PathBuf>,
}

impl CentralRepo {
    pub fn ledger(&self) -> MemoryFile {
        MemoryFile::new(self.dir.join(MEMORY_FILE))
    }

    /// Directory name, unique within the store.
    pub fn dir_name(&self) -> String {
        self.dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// The per-user central store.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Personal memories that apply beyond one repository.
    pub fn general(&self) -> MemoryFile {
        MemoryFile::new(self.root.join(GENERAL_DIR).join(MEMORY_FILE))
    }

    /// All registered central repositories, sorted by directory name.
    ///
    /// The same identity can appear twice after a sync until
    /// [`Store::register`] merges the directories.
    pub fn repos(&self) -> crate::Result<Vec<CentralRepo>> {
        let dir = self.root.join(REPOS_DIR);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let checkouts = self.local_checkouts()?;
        let mut repos = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let registry = entry.path().join(REGISTRY_FILE);
            if registry.is_file() {
                let record = read_record(&registry)?;
                repos.push(CentralRepo {
                    dir: entry.path(),
                    checkouts: checkouts.get(&record.identity).cloned().unwrap_or_default(),
                    record,
                });
            }
        }
        repos.sort_by(|a, b| a.dir.cmp(&b.dir));
        Ok(repos)
    }

    /// The central repo for `checkout`, if registered (the first directory
    /// when several share its identity).
    pub fn find(&self, checkout: &Checkout) -> crate::Result<Option<CentralRepo>> {
        Ok(self
            .repos()?
            .into_iter()
            .find(|repo| repo.record.identity == checkout.identity))
    }

    /// Where `checkout` is (or would be) stored, without writing anything.
    pub fn plan(&self, checkout: &Checkout) -> crate::Result<CentralRepo> {
        if let Some(repo) = self.find(checkout)? {
            return Ok(repo);
        }
        let name = format!(
            "{}--{}",
            sanitize(&checkout.name),
            short_id(&checkout.identity)
        );
        Ok(CentralRepo {
            dir: self.root.join(REPOS_DIR).join(name),
            record: RepoRecord {
                identity: checkout.identity.clone(),
                name: checkout.name.clone(),
            },
            checkouts: Vec::new(),
        })
    }

    /// Resolve and register `checkout`: create its directory, remember the
    /// checkout path locally, and merge duplicate directories of the same
    /// identity (from other machines) into the first one.
    pub fn register(&self, checkout: &Checkout) -> crate::Result<CentralRepo> {
        let mut repo = self.plan(checkout)?;
        let registry = repo.dir.join(REGISTRY_FILE);
        if !registry.is_file() {
            fs::create_dir_all(&repo.dir)?;
            write_atomic(&registry, &serde_json::to_string_pretty(&repo.record)?)?;
        }
        if !repo.checkouts.contains(&checkout.root) {
            repo.checkouts.push(checkout.root.clone());
            self.remember_checkout(&checkout.identity, &checkout.root)?;
        }
        self.merge_duplicates(&repo)?;
        Ok(repo)
    }

    /// Merge other directories with `primary`'s identity into it, then remove
    /// them. Directories with conflicting event ids are left in place.
    fn merge_duplicates(&self, primary: &CentralRepo) -> crate::Result<Vec<PathBuf>> {
        let mut kept = Vec::new();
        for other in self.repos()? {
            if other.record.identity != primary.record.identity || other.dir == primary.dir {
                continue;
            }
            let merged = merge_ledgers(&other.ledger(), &primary.ledger())?;
            if merged.conflicts.is_empty() {
                fs::remove_dir_all(&other.dir)?;
            } else {
                kept.push(other.dir);
            }
        }
        Ok(kept)
    }

    fn checkouts_file(&self) -> PathBuf {
        self.root.join(LOCAL_DIR).join(CHECKOUTS_FILE)
    }

    fn local_checkouts(&self) -> crate::Result<BTreeMap<String, Vec<PathBuf>>> {
        let path = self.checkouts_file();
        if !path.is_file() {
            return Ok(BTreeMap::new());
        }
        serde_json::from_str(&fs::read_to_string(&path)?)
            .map_err(|error| crate::Error::Config(format!("invalid {}: {error}", path.display())))
    }

    fn remember_checkout(&self, identity: &str, root: &Path) -> crate::Result<()> {
        let mut all = self.local_checkouts()?;
        let paths = all.entry(identity.to_owned()).or_default();
        if !paths.iter().any(|path| path == root) {
            paths.push(root.to_path_buf());
        }
        let path = self.checkouts_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_atomic(&path, &serde_json::to_string_pretty(&all)?)
    }
}

/// A repository checkout on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    /// Canonical checkout root.
    pub root: PathBuf,
    /// Directory name of the checkout.
    pub name: String,
    /// `git:<root-commit>` for git repos with history, else `path:<root>`.
    pub identity: String,
}

/// Why a repository uses its repo-local ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrackedReason {
    /// `.mmry/tracked` marker, created by `mmry init --tracked`.
    Marker,
    /// `.mmry/mmry.jsonl` is committed to git.
    GitTracked,
}

impl Checkout {
    /// The enclosing repository of `start`: nearest ancestor with `.mmry/`
    /// or `.git`. `None` outside any repository.
    pub fn detect(start: &Path) -> crate::Result<Option<Self>> {
        let Some(root) = start
            .ancestors()
            .find(|dir| dir.join(MMRY_DIR).is_dir())
            .or_else(|| start.ancestors().find(|dir| dir.join(".git").exists()))
        else {
            return Ok(None);
        };
        Self::at(root).map(Some)
    }

    /// Treat `root` as a checkout.
    pub fn at(root: &Path) -> crate::Result<Self> {
        let root = fs::canonicalize(root)?;
        let name = root
            .file_name()
            .map_or_else(|| "root".to_owned(), |n| n.to_string_lossy().into_owned());
        let identity = git_root_commit(&root).map_or_else(
            || format!("path:{}", root.display()),
            |sha| format!("git:{sha}"),
        );
        Ok(Self {
            root,
            name,
            identity,
        })
    }

    pub fn local_ledger(&self) -> MemoryFile {
        MemoryFile::open_at(&self.root)
    }

    /// `Some` when this checkout is in tracked (repo-local) mode.
    pub fn tracked(&self) -> Option<TrackedReason> {
        if self.root.join(MMRY_DIR).join(TRACKED_MARKER).exists() {
            Some(TrackedReason::Marker)
        } else if self.ledger_in_git() {
            Some(TrackedReason::GitTracked)
        } else {
            None
        }
    }

    fn ledger_in_git(&self) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["ls-files", "--error-unmatch", "--"])
            .arg(format!("{MMRY_DIR}/{MEMORY_FILE}"))
            .output()
            .is_ok_and(|output| output.status.success())
    }

    /// A repo-local ledger exists that belongs in the central store.
    pub fn needs_migration(&self) -> bool {
        self.tracked().is_none() && self.local_ledger().exists()
    }
}

/// Switch `root` to tracked mode: repo-local ledger plus marker.
pub fn init_tracked(root: &Path) -> crate::Result<MemoryFile> {
    let dir = root.join(MMRY_DIR);
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join(TRACKED_MARKER),
        "This repository keeps its mmry ledger in .mmry/mmry.jsonl (tracked mode).\n",
    )?;
    let ledger = MemoryFile::open_at(root);
    ledger.touch()?;
    Ok(ledger)
}

/// Options for [`migrate`].
#[derive(Debug, Clone, Copy, Default)]
pub struct MigrateOptions {
    pub dry_run: bool,
    /// Remove a git-tracked ledger from the index (`git rm --cached`) and
    /// migrate it. The resulting change is left uncommitted.
    pub untrack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationStatus {
    Migrated,
    NothingToMigrate,
    /// Ledger is committed to git; rerun with `untrack` to move it.
    TrackedInGit,
    /// Repo uses `.mmry/tracked`; never migrated.
    TrackedMode,
    /// Same event id with different content in both ledgers; nothing moved.
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationReport {
    pub repo_path: PathBuf,
    pub source: PathBuf,
    pub central: PathBuf,
    pub status: MigrationStatus,
    pub migrated: usize,
    pub skipped_duplicate: usize,
    pub conflicts: Vec<String>,
    pub backup: Option<PathBuf>,
    pub dry_run: bool,
}

/// Move `checkout`'s repo-local ledger into the central store.
///
/// Events are merged by id (idempotent), the central ledger is verified to
/// contain every active local memory, and only then is the local ledger
/// renamed to `mmry.jsonl.migrated-<ts>` (kept as backup) with a
/// `.mmry/MIGRATED` note. A crash before the rename leaves the source intact;
/// rerunning completes without duplicating events.
pub fn migrate(
    store: &Store,
    checkout: &Checkout,
    options: MigrateOptions,
) -> crate::Result<MigrationReport> {
    let local = checkout.local_ledger();
    let planned = store.plan(checkout)?;
    let mut report = MigrationReport {
        repo_path: checkout.root.clone(),
        source: local.path().to_path_buf(),
        central: planned.ledger().path().to_path_buf(),
        status: MigrationStatus::NothingToMigrate,
        migrated: 0,
        skipped_duplicate: 0,
        conflicts: Vec::new(),
        backup: None,
        dry_run: options.dry_run,
    };
    if !local.exists() {
        return Ok(report);
    }
    reject_symlinks(checkout)?;
    match checkout.tracked() {
        Some(TrackedReason::Marker) => {
            report.status = MigrationStatus::TrackedMode;
            return Ok(report);
        }
        Some(TrackedReason::GitTracked) if !options.untrack => {
            report.status = MigrationStatus::TrackedInGit;
            return Ok(report);
        }
        _ => {}
    }

    let lock = fs::OpenOptions::new().read(true).open(local.path())?;
    lock.lock_exclusive()?;
    let result = merge_into(store, checkout, &local, &planned, options, &mut report);
    lock.unlock()?;
    drop(lock);
    result?;

    if options.dry_run || report.status == MigrationStatus::Conflict {
        return Ok(report);
    }
    if checkout.tracked() == Some(TrackedReason::GitTracked) {
        untrack(checkout)?;
    }
    let backup = local.path().with_file_name(format!(
        "{MEMORY_FILE}.migrated-{}",
        Utc::now().format("%Y%m%dT%H%M%S%.fZ")
    ));
    fs::rename(local.path(), &backup)?;
    fs::write(
        checkout.root.join(MMRY_DIR).join(MIGRATED_MARKER),
        format!(
            "Memories of this repository live in {}\nBackup of the migrated repo-local ledger: {}\n",
            report.central.display(),
            backup.display()
        ),
    )?;
    report.backup = Some(backup);
    report.status = MigrationStatus::Migrated;
    Ok(report)
}

fn merge_into(
    store: &Store,
    checkout: &Checkout,
    local: &MemoryFile,
    planned: &CentralRepo,
    options: MigrateOptions,
    report: &mut MigrationReport,
) -> crate::Result<()> {
    let local_lines = local.read_lines()?;
    let central_existing: HashMap<String, Value> = planned
        .ledger()
        .read_lines()?
        .into_iter()
        .map(|(line, event)| Ok((event.id, serde_json::from_str(&line)?)))
        .collect::<crate::Result<_>>()?;

    let mut pending = Vec::new();
    let mut pending_ids = HashSet::new();
    for (line, event) in &local_lines {
        let value: Value = serde_json::from_str(line)?;
        match central_existing.get(&event.id) {
            Some(existing) if *existing == value => report.skipped_duplicate += 1,
            Some(_) => report.conflicts.push(event.id.clone()),
            None if pending_ids.insert(event.id.clone()) => pending.push(line.clone()),
            None => report.skipped_duplicate += 1,
        }
    }
    report.migrated = pending.len();
    if !report.conflicts.is_empty() {
        report.status = MigrationStatus::Conflict;
        return Ok(());
    }
    if options.dry_run {
        report.status = MigrationStatus::Migrated;
        return Ok(());
    }

    let central = store.register(checkout)?.ledger();
    central.append_raw_lines(&pending)?;
    verify_contains(&central, local_lines.into_iter().map(|(_, e)| e).collect())
}

/// Outcome of merging one ledger's events into another.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    pub added: usize,
    pub duplicate: usize,
    /// Event ids present in both ledgers with different content.
    pub conflicts: Vec<String>,
}

/// Append events of `from` missing in `into`, byte-for-byte, under the
/// target's lock. Nothing is written if any event id conflicts.
pub fn merge_ledgers(from: &MemoryFile, into: &MemoryFile) -> crate::Result<MergeOutcome> {
    let existing: HashMap<String, Value> = into
        .read_lines()?
        .into_iter()
        .map(|(line, event)| Ok((event.id, serde_json::from_str(&line)?)))
        .collect::<crate::Result<_>>()?;
    let mut outcome = MergeOutcome::default();
    let mut pending = Vec::new();
    let mut seen = HashSet::new();
    for (line, event) in from.read_lines()? {
        let value: Value = serde_json::from_str(&line)?;
        match existing.get(&event.id) {
            Some(current) if *current == value => outcome.duplicate += 1,
            Some(_) => outcome.conflicts.push(event.id),
            None if seen.insert(event.id) => pending.push(line),
            None => outcome.duplicate += 1,
        }
    }
    if outcome.conflicts.is_empty() {
        outcome.added = pending.len();
        into.append_raw_lines(&pending)?;
    }
    Ok(outcome)
}

/// Every active local memory must be active centrally with the same content.
fn verify_contains(central: &MemoryFile, local_events: Vec<MemoryEvent>) -> crate::Result<()> {
    let central_active: HashMap<_, _> = central
        .active_memories()?
        .into_iter()
        .map(|entry| (entry.memory_id.clone(), entry.content))
        .collect();
    for entry in project(local_events) {
        if central_active.get(&entry.memory_id) != Some(&entry.content) {
            return Err(crate::Error::InvalidInput(format!(
                "migration verification failed for {} in {}; the repo-local ledger was left in place",
                entry.memory_id,
                central.path().display()
            )));
        }
    }
    Ok(())
}

fn reject_symlinks(checkout: &Checkout) -> crate::Result<()> {
    let dir = checkout.root.join(MMRY_DIR);
    for path in [dir.clone(), dir.join(MEMORY_FILE)] {
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(crate::Error::InvalidInput(format!(
                "refusing to migrate through symlink {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn untrack(checkout: &Checkout) -> crate::Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(&checkout.root)
        .args(["rm", "--cached", "--quiet", "--"])
        .arg(format!("{MMRY_DIR}/{MEMORY_FILE}"))
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(crate::Error::InvalidInput(format!(
            "git rm --cached failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn git_root_commit(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-list", "--max-parents=0", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .min()
        .map(str::to_owned)
}

fn read_record(path: &Path) -> crate::Result<RepoRecord> {
    serde_json::from_str(&fs::read_to_string(path)?).map_err(|error| {
        crate::Error::Config(format!("invalid registry {}: {error}", path.display()))
    })
}

fn write_atomic(path: &Path, content: &str) -> crate::Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&tmp, content)?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn sanitize(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = clean.trim_start_matches('.');
    if clean.is_empty() {
        "repo".to_owned()
    } else {
        clean.to_owned()
    }
}

/// Short, stable id suffix of every central repository directory.
fn short_id(identity: &str) -> String {
    if let Some(sha) = identity.strip_prefix("git:") {
        return sha.chars().take(12).collect();
    }
    // FNV-1a: stable across runs and platforms, unlike `DefaultHasher`.
    let hash = identity
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{hash:016x}").chars().take(12).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentCtx;
    use crate::MemoryType;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(status.status.success(), "git {args:?}: {status:?}");
    }

    /// A git repository with one commit (distinct root commit per call).
    fn git_repo(parent: &Path, name: &str) -> PathBuf {
        let path = parent.join(name);
        fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-q"]);
        fs::write(path.join("README"), format!("{}\n", uuid::Uuid::new_v4())).unwrap();
        git(&path, &["add", "README"]);
        git(&path, &["commit", "-q", "-m", "init"]);
        path
    }

    fn add(file: &MemoryFile, text: &str) -> MemoryEvent {
        let event = MemoryEvent::add(
            text.into(),
            MemoryType::Semantic,
            Vec::new(),
            &AgentCtx::default(),
        );
        file.append(&event).unwrap();
        event
    }

    fn contents(file: &MemoryFile) -> Vec<String> {
        let mut items: Vec<_> = file
            .active_memories()
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .collect();
        items.sort();
        items
    }

    struct Fixture {
        _work: tempfile::TempDir,
        _state: tempfile::TempDir,
        work: PathBuf,
        store: Store,
    }

    fn fixture() -> Fixture {
        let work = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        Fixture {
            work: work.path().to_path_buf(),
            store: Store::new(state.path()),
            _work: work,
            _state: state,
        }
    }

    #[test]
    fn detect_finds_enclosing_repo_and_git_identity() {
        let fx = fixture();
        let repo = git_repo(&fx.work, "app");
        let nested = repo.join("src/deep");
        fs::create_dir_all(&nested).unwrap();
        let checkout = Checkout::detect(&nested).unwrap().unwrap();
        assert_eq!(checkout.root, fs::canonicalize(&repo).unwrap());
        assert_eq!(checkout.name, "app");
        assert!(
            checkout.identity.starts_with("git:"),
            "{}",
            checkout.identity
        );

        let plain = fx.work.join("plain");
        fs::create_dir_all(&plain).unwrap();
        let plain = Checkout::at(&plain).unwrap();
        assert!(plain.identity.starts_with("path:"));
    }

    #[test]
    fn clones_share_a_central_repo_and_same_names_do_not_merge() {
        let fx = fixture();
        let first = git_repo(&fx.work.join("a"), "app");
        let other = git_repo(&fx.work.join("b"), "app");
        let clone = fx.work.join("clone/app");
        git(
            &fx.work,
            &[
                "clone",
                "-q",
                first.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );

        let first = Checkout::at(&first).unwrap();
        let clone = Checkout::at(&clone).unwrap();
        let other = Checkout::at(&other).unwrap();
        let a = fx.store.register(&first).unwrap();
        let c = fx.store.register(&clone).unwrap();
        let o = fx.store.register(&other).unwrap();

        assert_eq!(a.dir, c.dir);
        assert_eq!(
            fx.store.find(&first).unwrap().unwrap().checkouts,
            vec![first.root.clone(), clone.root]
        );
        // Names never depend on registration order: always <name>--<id>.
        let id = |c: &Checkout| c.identity.trim_start_matches("git:")[..12].to_owned();
        assert_eq!(a.dir_name(), format!("app--{}", id(&first)));
        assert_eq!(o.dir_name(), format!("app--{}", id(&other)));
        assert_eq!(fx.store.repos().unwrap().len(), 2);
        // Checkout paths are machine-local; repo.json holds only identity + name.
        let record = fs::read_to_string(a.dir.join(REGISTRY_FILE)).unwrap();
        assert!(!record.contains(fx.work.to_str().unwrap()), "{record}");
    }

    #[test]
    fn registration_order_does_not_change_directory_names() {
        let fx = fixture();
        let x = Checkout::at(&git_repo(&fx.work.join("work"), "app")).unwrap();
        let y = Checkout::at(&git_repo(&fx.work.join("byteowlz"), "app")).unwrap();
        let other = Store::new(fx.work.join("other-machine"));
        let here = (
            fx.store.register(&x).unwrap(),
            fx.store.register(&y).unwrap(),
        );
        let there = (other.register(&y).unwrap(), other.register(&x).unwrap());
        assert_eq!(here.0.dir_name(), there.1.dir_name());
        assert_eq!(here.1.dir_name(), there.0.dir_name());
        assert_ne!(here.0.dir_name(), here.1.dir_name());
    }

    #[test]
    fn synced_duplicate_directories_are_merged_by_event_id() {
        let fx = fixture();
        let repo = git_repo(&fx.work, "app");
        let here = Checkout::at(&repo).unwrap();
        // The other machine cloned the same repository as "app-fork".
        let there = Checkout {
            name: "app-fork".into(),
            ..here.clone()
        };
        let other = Store::new(fx.work.join("other-machine"));
        add(&fx.store.register(&here).unwrap().ledger(), "from here");
        let remote = other.register(&there).unwrap();
        add(&remote.ledger(), "from there");
        // Simulate sync: the other machine's directory appears here.
        let synced = fx.store.root().join(REPOS_DIR).join(remote.dir_name());
        fs::create_dir_all(&synced).unwrap();
        for file in [MEMORY_FILE, REGISTRY_FILE] {
            fs::copy(remote.dir.join(file), synced.join(file)).unwrap();
        }
        assert_eq!(fx.store.repos().unwrap().len(), 2);

        let merged = fx.store.register(&here).unwrap();
        assert_eq!(fx.store.repos().unwrap().len(), 1);
        assert_eq!(contents(&merged.ledger()), ["from here", "from there"]);
    }

    #[test]
    fn plan_does_not_write() {
        let fx = fixture();
        let checkout = Checkout::at(&git_repo(&fx.work, "app")).unwrap();
        let planned = fx.store.plan(&checkout).unwrap();
        assert!(!planned.dir.exists());
        assert!(fx.store.repos().unwrap().is_empty());
    }

    #[test]
    fn migrate_moves_events_keeps_backup_and_is_idempotent() {
        let fx = fixture();
        let repo = git_repo(&fx.work, "app");
        let checkout = Checkout::at(&repo).unwrap();
        let local = checkout.local_ledger();
        add(&local, "one");
        let gone = add(&local, "two");
        local
            .append(&MemoryEvent::deprecate(
                gone.memory_id,
                &AgentCtx::default(),
            ))
            .unwrap();
        // Unknown fields must survive byte-for-byte.
        let mut extra =
            serde_json::to_value(add(&MemoryFile::new(fx.work.join("x")), "three")).unwrap();
        extra["future_field"] = serde_json::json!({"k": 1});
        fs::write(
            local.path(),
            fs::read_to_string(local.path()).unwrap() + &extra.to_string() + "\n",
        )
        .unwrap();
        assert!(checkout.needs_migration());

        let report = migrate(&fx.store, &checkout, MigrateOptions::default()).unwrap();
        assert_eq!(report.status, MigrationStatus::Migrated);
        assert_eq!((report.migrated, report.skipped_duplicate), (4, 0));
        let central = fx.store.find(&checkout).unwrap().unwrap().ledger();
        assert_eq!(contents(&central), ["one", "three"]);
        assert!(
            fs::read_to_string(central.path())
                .unwrap()
                .contains("future_field")
        );
        assert!(!local.exists());
        assert!(report.backup.as_ref().unwrap().exists());
        assert!(
            fs::read_to_string(repo.join(".mmry/MIGRATED"))
                .unwrap()
                .contains(&central.path().display().to_string())
        );
        assert!(!checkout.needs_migration());

        let rerun = migrate(&fx.store, &checkout, MigrateOptions::default()).unwrap();
        assert_eq!(rerun.status, MigrationStatus::NothingToMigrate);
        assert_eq!(contents(&central), ["one", "three"]);
    }

    #[test]
    fn stale_local_ledger_after_migration_is_merged_without_duplicates() {
        let fx = fixture();
        let checkout = Checkout::at(&git_repo(&fx.work, "app")).unwrap();
        let local = checkout.local_ledger();
        add(&local, "one");
        let report = migrate(&fx.store, &checkout, MigrateOptions::default()).unwrap();
        // An old branch brings the ledger back with one extra event.
        fs::copy(report.backup.unwrap(), local.path()).unwrap();
        add(&local, "two");
        let again = migrate(&fx.store, &checkout, MigrateOptions::default()).unwrap();
        assert_eq!((again.migrated, again.skipped_duplicate), (1, 1));
        let central = fx.store.find(&checkout).unwrap().unwrap().ledger();
        assert_eq!(contents(&central), ["one", "two"]);
        assert_eq!(central.read_events().unwrap().len(), 2);
    }

    #[test]
    fn interrupted_migration_completes_on_rerun() {
        let fx = fixture();
        let checkout = Checkout::at(&git_repo(&fx.work, "app")).unwrap();
        let local = checkout.local_ledger();
        add(&local, "one");
        add(&local, "two");
        // Simulate a crash after the merge but before the rename.
        let central = fx.store.register(&checkout).unwrap().ledger();
        fs::copy(local.path(), central.path()).unwrap();

        let report = migrate(&fx.store, &checkout, MigrateOptions::default()).unwrap();
        assert_eq!(report.status, MigrationStatus::Migrated);
        assert_eq!((report.migrated, report.skipped_duplicate), (0, 2));
        assert_eq!(central.read_events().unwrap().len(), 2);
        assert!(!local.exists());
    }

    #[test]
    fn conflicting_event_id_aborts_and_leaves_source() {
        let fx = fixture();
        let checkout = Checkout::at(&git_repo(&fx.work, "app")).unwrap();
        let local = checkout.local_ledger();
        let event = add(&local, "local text");
        let central = fx.store.register(&checkout).unwrap().ledger();
        let mut clash = event.clone();
        clash.content = Some("different text".into());
        central.append(&clash).unwrap();
        let before = fs::read_to_string(central.path()).unwrap();

        let report = migrate(&fx.store, &checkout, MigrateOptions::default()).unwrap();
        assert_eq!(report.status, MigrationStatus::Conflict);
        assert_eq!(report.conflicts, vec![event.id]);
        assert!(local.exists());
        assert_eq!(fs::read_to_string(central.path()).unwrap(), before);
    }

    #[test]
    fn dry_run_writes_nothing() {
        let fx = fixture();
        let checkout = Checkout::at(&git_repo(&fx.work, "app")).unwrap();
        add(&checkout.local_ledger(), "one");
        let report = migrate(
            &fx.store,
            &checkout,
            MigrateOptions {
                dry_run: true,
                untrack: false,
            },
        )
        .unwrap();
        assert_eq!(
            (report.status, report.migrated),
            (MigrationStatus::Migrated, 1)
        );
        assert!(report.backup.is_none());
        assert!(checkout.local_ledger().exists());
        assert!(!fx.store.root().join(REPOS_DIR).exists());
    }

    #[test]
    fn tracked_ledgers_stay_unless_untracked() {
        let fx = fixture();
        let repo = git_repo(&fx.work, "app");
        let checkout = Checkout::at(&repo).unwrap();
        add(&checkout.local_ledger(), "committed");
        git(&repo, &["add", "-f", ".mmry/mmry.jsonl"]);
        git(&repo, &["commit", "-q", "-m", "ledger"]);
        assert_eq!(checkout.tracked(), Some(TrackedReason::GitTracked));
        assert!(!checkout.needs_migration());

        let report = migrate(&fx.store, &checkout, MigrateOptions::default()).unwrap();
        assert_eq!(report.status, MigrationStatus::TrackedInGit);
        assert!(checkout.local_ledger().exists());

        let options = MigrateOptions {
            dry_run: false,
            untrack: true,
        };
        let report = migrate(&fx.store, &checkout, options).unwrap();
        assert_eq!(report.status, MigrationStatus::Migrated);
        assert_eq!(checkout.tracked(), None);
        let central = fx.store.find(&checkout).unwrap().unwrap().ledger();
        assert_eq!(contents(&central), ["committed"]);
    }

    #[test]
    fn tracked_marker_is_never_migrated() {
        let fx = fixture();
        let repo = git_repo(&fx.work, "app");
        add(&init_tracked(&repo).unwrap(), "stays");
        let checkout = Checkout::at(&repo).unwrap();
        assert_eq!(checkout.tracked(), Some(TrackedReason::Marker));
        let report = migrate(&fx.store, &checkout, MigrateOptions::default()).unwrap();
        assert_eq!(report.status, MigrationStatus::TrackedMode);
        assert!(checkout.local_ledger().exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_ledger_is_refused() {
        let fx = fixture();
        let repo = git_repo(&fx.work, "app");
        let outside = MemoryFile::new(fx.work.join("outside.jsonl"));
        add(&outside, "elsewhere");
        fs::create_dir_all(repo.join(MMRY_DIR)).unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.join(".mmry/mmry.jsonl")).unwrap();
        let checkout = Checkout::at(&repo).unwrap();
        let error = migrate(&fx.store, &checkout, MigrateOptions::default()).unwrap_err();
        assert!(error.to_string().contains("symlink"), "{error}");
        assert!(outside.exists());
    }

    #[test]
    fn sanitize_and_short_id_are_stable() {
        assert_eq!(sanitize("my app!"), "my_app_");
        assert_eq!(sanitize(".."), "repo");
        assert_eq!(short_id("git:0123456789abcdef"), "0123456789ab");
        assert_eq!(short_id("path:/x"), short_id("path:/x"));
        assert_ne!(short_id("path:/x"), short_id("path:/y"));
    }
}
