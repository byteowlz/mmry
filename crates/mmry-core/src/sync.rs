//! Opt-in git sync of the central store.
//!
//! The store becomes a git repository. Ledgers merge with git's
//! `merge=union` driver: events are append-only lines with unique ids and
//! replay sorts them, so keeping both sides' lines is a correct merge.
//! Semantic conflicts (two machines editing one memory) surface as contested
//! memories on replay. `local/` holds machine-local state and is ignored.
//!
//! Every git call is non-interactive and bounded by a timeout. Failures
//! (offline, rejected push, merge conflict outside ledgers) never drop local
//! writes: commits stay local and are reported as pending. There is no
//! force-push and no reset. Deprecated content stays in git history: `rm`
//! is not erasure.

use crate::store::LOCAL_DIR;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

/// Branch used on every machine.
pub const BRANCH: &str = "main";
const REMOTE: &str = "origin";
const STATE_FILE: &str = "sync.json";

pub const GITATTRIBUTES: &str = "\
# mmry ledgers are append-only JSONL with unique event ids: keep both sides.
**/mmry.jsonl text eol=lf merge=union
";

pub const GITIGNORE: &str = "\
# Only ledgers and repository metadata are synced. Everything else in the
# store (local/, files of other tools or older mmry versions) is machine-local.
/*
!/.gitattributes
!/.gitignore
!/general/
!/repos/
*.tmp
*.tmp-*
";

/// Whether a store path (relative, `/`-separated) is synced: the rule that
/// [`GITIGNORE`] encodes, used to untrack files committed by older versions.
fn synced(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    let temporary = Path::new(file)
        .extension()
        .is_some_and(|extension| extension == "tmp")
        || file.contains(".tmp-");
    !temporary
        && (path == ".gitattributes"
            || path == ".gitignore"
            || path.starts_with("general/")
            || path.starts_with("repos/"))
}

/// Last sync outcome, stored machine-locally in `local/sync.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    pub last_pull: Option<DateTime<Utc>>,
    pub last_push: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// Output of `mmry sync status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncStatus {
    pub enabled: bool,
    pub root: PathBuf,
    pub remote: Option<String>,
    /// Local commits not on the remote-tracking branch (as of the last fetch).
    pub pending_commits: usize,
    /// Remote commits not merged yet (as of the last fetch).
    pub behind: usize,
    /// Uncommitted changes in the store.
    pub uncommitted: bool,
    /// A merge is in progress (should not happen; mmry aborts conflicts).
    pub merging: bool,
    #[serde(flatten)]
    pub state: SyncState,
}

/// What a pull/push/sync did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncOutcome {
    pub committed: bool,
    pub pulled: bool,
    pub pushed: bool,
    /// Set when the remote could not be reached or refused; local data kept.
    pub error: Option<String>,
    pub status: SyncStatus,
}

/// Git sync of one store.
#[derive(Debug, Clone)]
pub struct Sync {
    root: PathBuf,
    timeout: Duration,
    machine: String,
}

struct GitOutput {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Sync {
    pub fn new(root: impl Into<PathBuf>, timeout: Duration, machine: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            timeout,
            machine: machine.into(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.root.join(".git").exists()
    }

    /// Make the store a git repository (idempotent), write
    /// `.gitattributes`/`.gitignore`, commit existing ledgers and, with a
    /// remote, merge its history (unrelated histories allowed: ledgers from
    /// both machines are unioned) and push.
    pub fn init(&self, remote: Option<&str>) -> crate::Result<SyncOutcome> {
        fs::create_dir_all(&self.root)?;
        if !self.is_enabled() {
            self.git_ok(&["init", "-q", "-b", BRANCH])?;
        }
        if let Some(url) = remote {
            if self.remote_url()?.is_some() {
                self.git_ok(&["remote", "set-url", REMOTE, url])?;
            } else {
                self.git_ok(&["remote", "add", REMOTE, url])?;
            }
        }
        self.commit()?;
        if self.remote_url()?.is_none() {
            return Ok(SyncOutcome {
                committed: true,
                pulled: false,
                pushed: false,
                error: None,
                status: self.status()?,
            });
        }
        self.sync()
    }

    /// Commit, pull, then push.
    pub fn sync(&self) -> crate::Result<SyncOutcome> {
        let committed = self.commit()?;
        let pulled = self.pull_inner();
        let pushed = match &pulled {
            Ok(()) => self.push_inner(),
            Err(error) => Err(error.clone()),
        };
        self.finish(committed, pulled.is_ok(), pushed)
    }

    pub fn pull(&self) -> crate::Result<SyncOutcome> {
        let committed = self.commit()?;
        let pulled = self.pull_inner();
        let error = pulled.as_ref().err().cloned();
        let mut state = self.read_state()?;
        if pulled.is_ok() {
            state.last_pull = Some(Utc::now());
        }
        state.last_error.clone_from(&error);
        self.write_state(&state)?;
        Ok(SyncOutcome {
            committed,
            pulled: pulled.is_ok(),
            pushed: false,
            error,
            status: self.status()?,
        })
    }

    pub fn push(&self) -> crate::Result<SyncOutcome> {
        let committed = self.commit()?;
        let pushed = self.push_inner();
        self.finish(committed, false, pushed)
    }

    fn finish(
        &self,
        committed: bool,
        pulled: bool,
        pushed: Result<(), String>,
    ) -> crate::Result<SyncOutcome> {
        let mut state = self.read_state()?;
        let now = Utc::now();
        if pulled {
            state.last_pull = Some(now);
        }
        if pushed.is_ok() {
            state.last_push = Some(now);
        }
        let error = pushed.err();
        state.last_error.clone_from(&error);
        self.write_state(&state)?;
        Ok(SyncOutcome {
            committed,
            pulled,
            pushed: error.is_none(),
            error,
            status: self.status()?,
        })
    }

    /// Commit every change in the store. Returns whether a commit was made.
    pub fn commit(&self) -> crate::Result<bool> {
        if !self.is_enabled() {
            return Err(not_enabled());
        }
        // Keep the rules current so stores set up by older versions heal.
        for (name, content) in [(".gitattributes", GITATTRIBUTES), (".gitignore", GITIGNORE)] {
            let path = self.root.join(name);
            if fs::read_to_string(&path).ok().as_deref() != Some(content) {
                fs::write(path, content)?;
            }
        }
        self.git_ok(&["add", "-A"])?;
        self.untrack_unsynced()?;
        let staged = self.git(&["diff", "--cached", "--quiet"])?;
        if staged.ok {
            return Ok(false);
        }
        self.commit_staged(&format!("mmry: {}", self.machine))?;
        Ok(true)
    }

    /// Remove tracked paths outside the synced set from the index (files
    /// stay on disk).
    fn untrack_unsynced(&self) -> crate::Result<()> {
        let tracked = self.git_ok(&["ls-files", "-z"])?;
        let unsynced: Vec<&str> = tracked
            .split('\0')
            .filter(|p| !p.is_empty() && !synced(p))
            .collect();
        if !unsynced.is_empty() {
            let mut args = vec!["rm", "--cached", "-q", "--"];
            args.extend(unsynced);
            self.git_ok(&args)?;
        }
        Ok(())
    }

    /// Unmerged paths missing on one side (modify/delete conflicts).
    fn one_sided_conflicts(&self) -> crate::Result<Vec<String>> {
        let mut stages: BTreeMap<String, Vec<char>> = BTreeMap::new();
        for line in self.git_ok(&["ls-files", "-u", "-z"])?.split('\0') {
            // "<mode> <object> <stage>\t<path>"
            if let Some((meta, path)) = line.split_once('\t')
                && let Some(stage) = meta.chars().last()
            {
                stages.entry(path.to_owned()).or_default().push(stage);
            }
        }
        Ok(stages
            .into_iter()
            .filter(|(_, stages)| !(stages.contains(&'2') && stages.contains(&'3')))
            .map(|(path, _)| path)
            .collect())
    }

    fn commit_staged(&self, message: &str) -> crate::Result<()> {
        let identity_known = self.git(&["config", "user.email"])?.ok;
        let mut args = vec!["-c", "commit.gpgsign=false"];
        let email = format!("user.email=mmry@{}", self.machine);
        if !identity_known {
            args.extend(["-c", "user.name=mmry", "-c", email.as_str()]);
        }
        args.extend(["commit", "-q", "--no-edit", "-m", message]);
        self.git_ok(&args)?;
        Ok(())
    }

    fn pull_inner(&self) -> Result<(), String> {
        let fetch = self
            .git(&["fetch", "-q", REMOTE])
            .map_err(|error| error.to_string())?;
        if !fetch.ok {
            return Err(format!("fetch failed: {}", fetch.stderr.trim()));
        }
        let remote_ref = format!("{REMOTE}/{BRANCH}");
        if !self
            .git(&["rev-parse", "--verify", "-q", &remote_ref])
            .map_err(|error| error.to_string())?
            .ok
        {
            return Ok(()); // empty remote
        }
        // Machine-local files another machine committed (older versions
        // synced everything) must neither block nor overwrite ours: move local
        // copies aside, merge, drop those paths from tracking, restore.
        let theirs = self
            .git_ok(&["ls-tree", "-r", "-z", "--name-only", &remote_ref])
            .map_err(|error| error.to_string())?;
        let foreign: Vec<String> = theirs
            .split('\0')
            .filter(|p| !p.is_empty() && !synced(p))
            .map(str::to_owned)
            .collect();
        let mut saved = Vec::new();
        for path in &foreign {
            let file = self.root.join(path);
            if let Ok(bytes) = fs::read(&file) {
                saved.push((file.clone(), bytes));
                let _ = fs::remove_file(&file);
            }
        }
        let result = self.merge(&remote_ref, &foreign);
        for (file, bytes) in saved {
            if let Some(parent) = file.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(file, bytes);
        }
        result
    }

    fn merge(&self, remote_ref: &str, foreign: &[String]) -> Result<(), String> {
        let merge = self
            .git(&[
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=mmry",
                "-c",
                &format!("user.email=mmry@{}", self.machine),
                "merge",
                "-q",
                "--no-edit",
                "--allow-unrelated-histories",
                remote_ref,
            ])
            .map_err(|error| error.to_string())?;
        let healed = |sync: &Self| -> crate::Result<()> {
            sync.untrack_unsynced()?;
            if !sync.git(&["diff", "--cached", "--quiet"])?.ok {
                sync.commit_staged(&format!(
                    "mmry: {} (untrack machine-local files)",
                    sync.machine
                ))?;
            }
            Ok(())
        };
        if merge.ok {
            return healed(self).map_err(|error| error.to_string());
        }
        let conflicts = self
            .git_ok(&["diff", "--name-only", "--diff-filter=U", "-z"])
            .map_err(|error| error.to_string())?;
        let conflicts: Vec<&str> = conflicts.split('\0').filter(|p| !p.is_empty()).collect();
        // Resolvable: machine-local files (drop them), the rule files mmry
        // owns (write the current version) and synced files one side deleted
        // while the other changed them (keep them: a duplicate directory merged
        // on one machine while the other still wrote to it; the next register
        // merges it again). Anything else aborts.
        let rules = [(".gitattributes", GITATTRIBUTES), (".gitignore", GITIGNORE)];
        let deleted_on_one_side = self
            .one_sided_conflicts()
            .map_err(|error| error.to_string())?;
        let (owned, rest): (Vec<&str>, Vec<&str>) = conflicts
            .iter()
            .partition(|p| rules.iter().any(|(name, _)| name == *p));
        let (keep, local): (Vec<&str>, Vec<&str>) = rest
            .into_iter()
            .partition(|p| synced(p) && deleted_on_one_side.iter().any(|d| d == p));
        if !conflicts.is_empty() && local.iter().all(|p| foreign.iter().any(|f| f == p)) {
            let resolved = (|| -> crate::Result<()> {
                for (name, content) in rules.iter().filter(|(name, _)| owned.contains(name)) {
                    fs::write(self.root.join(name), content)?;
                    self.git_ok(&["add", "--", name])?;
                }
                if !keep.is_empty() {
                    let mut args = vec!["add", "--"];
                    args.extend(&keep);
                    self.git_ok(&args)?;
                }
                if !local.is_empty() {
                    let mut args = vec!["rm", "-q", "-f", "--"];
                    args.extend(&local);
                    self.git_ok(&args)?;
                }
                self.commit_staged(&format!("mmry: merge {remote_ref}"))?;
                healed(self)
            })();
            if resolved.is_ok() {
                return Ok(());
            }
        }
        // Never leave a half-merged store: abort and report.
        let _ = self.git(&["merge", "--abort"]);
        Err(format!(
            "merge conflict outside ledgers; merge aborted, local data kept: {}",
            merge.stdout.trim()
        ))
    }

    fn push_inner(&self) -> Result<(), String> {
        let push = || {
            self.git(&["push", "-q", "-u", REMOTE, BRANCH])
                .map_err(|error| error.to_string())
        };
        let first = push()?;
        if first.ok {
            return Ok(());
        }
        let rejected = first.stderr.contains("rejected") || first.stderr.contains("fetch first");
        if !rejected {
            return Err(format!("push failed: {}", first.stderr.trim()));
        }
        self.pull_inner()?;
        let second = push()?;
        if second.ok {
            Ok(())
        } else {
            Err(format!("push failed after pull: {}", second.stderr.trim()))
        }
    }

    pub fn status(&self) -> crate::Result<SyncStatus> {
        let state = self.read_state()?;
        if !self.is_enabled() {
            return Ok(SyncStatus {
                enabled: false,
                root: self.root.clone(),
                remote: None,
                pending_commits: 0,
                behind: 0,
                uncommitted: false,
                merging: false,
                state,
            });
        }
        let remote_ref = format!("{REMOTE}/{BRANCH}");
        let has_remote_ref = self.git(&["rev-parse", "--verify", "-q", &remote_ref])?.ok;
        let count = |range: &str| -> crate::Result<usize> {
            let out = self.git(&["rev-list", "--count", range])?;
            Ok(out.stdout.trim().parse().unwrap_or(0))
        };
        let has_head = self.git(&["rev-parse", "--verify", "-q", "HEAD"])?.ok;
        let (pending_commits, behind) = match (has_head, has_remote_ref) {
            (true, true) => (
                count(&format!("{remote_ref}..HEAD"))?,
                count(&format!("HEAD..{remote_ref}"))?,
            ),
            (true, false) => (count("HEAD")?, 0),
            _ => (0, 0),
        };
        Ok(SyncStatus {
            enabled: true,
            root: self.root.clone(),
            remote: self.remote_url()?,
            pending_commits,
            behind,
            uncommitted: !self
                .git(&["status", "--porcelain"])?
                .stdout
                .trim()
                .is_empty(),
            merging: self.root.join(".git/MERGE_HEAD").exists(),
            state,
        })
    }

    fn remote_url(&self) -> crate::Result<Option<String>> {
        let out = self.git(&["remote", "get-url", REMOTE])?;
        Ok(out.ok.then(|| out.stdout.trim().to_owned()))
    }

    fn state_path(&self) -> PathBuf {
        self.root.join(LOCAL_DIR).join(STATE_FILE)
    }

    fn read_state(&self) -> crate::Result<SyncState> {
        let path = self.state_path();
        if !path.is_file() {
            return Ok(SyncState::default());
        }
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }

    fn write_state(&self, state: &SyncState) -> crate::Result<()> {
        let path = self.state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(state)?)?;
        Ok(())
    }

    fn git_ok(&self, args: &[&str]) -> crate::Result<String> {
        let out = self.git(args)?;
        if out.ok {
            Ok(out.stdout)
        } else {
            Err(crate::Error::InvalidInput(format!(
                "git {} failed: {}",
                args.join(" "),
                out.stderr.trim()
            )))
        }
    }

    /// Run git non-interactively in the store, killed after the timeout.
    fn git(&self, args: &[&str]) -> crate::Result<GitOutput> {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_SSH_COMMAND", ssh_command())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut text = String::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_string(&mut text);
                }
                text
            })
        };
        let stdout = drain(
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        );
        let stderr = drain(
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        );
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if start.elapsed() >= self.timeout {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let stdout = stdout.join().unwrap_or_default();
        let stderr = stderr.join().unwrap_or_default();
        Ok(match status {
            Some(status) => GitOutput {
                ok: status.success(),
                stdout,
                stderr,
            },
            None => GitOutput {
                ok: false,
                stdout,
                stderr: format!("timed out after {}s", self.timeout.as_secs()),
            },
        })
    }
}

/// Keep the user's ssh command, but never prompt.
fn ssh_command() -> String {
    std::env::var("GIT_SSH_COMMAND").unwrap_or_else(|_| "ssh -o BatchMode=yes".to_owned())
}

fn not_enabled() -> crate::Error {
    crate::Error::InvalidInput("sync is not set up; run `mmry sync init [--remote URL]`".into())
}

/// Whether `root` contains a git repository (for callers without a `Sync`).
pub fn is_enabled(root: &Path) -> bool {
    root.join(".git").exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentCtx;
    use crate::MemoryEvent;
    use crate::MemoryFile;
    use crate::MemoryType;
    use crate::store::Store;
    use std::collections::HashMap;

    struct Machines {
        _dir: tempfile::TempDir,
        remote: PathBuf,
        a: (Store, Sync),
        b: (Store, Sync),
    }

    fn machines() -> Machines {
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        let status = Command::new("git")
            .args(["init", "-q", "--bare", "-b", BRANCH])
            .arg(&remote)
            .status()
            .unwrap();
        assert!(status.success());
        let machine = |name: &str| {
            let root = dir.path().join(name);
            (
                Store::new(&root),
                Sync::new(&root, Duration::from_secs(20), name),
            )
        };
        Machines {
            a: machine("a"),
            b: machine("b"),
            remote,
            _dir: dir,
        }
    }

    fn note(store: &Store, text: &str) -> String {
        let event = MemoryEvent::add(
            text.into(),
            MemoryType::Semantic,
            Vec::new(),
            &AgentCtx::default(),
        );
        store.general().append(&event).unwrap();
        event.id
    }

    /// Event id -> number of lines carrying it.
    fn event_counts(store: &Store) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        for line in fs::read_to_string(store.general().path()).unwrap().lines() {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            *counts
                .entry(value["id"].as_str().unwrap().to_owned())
                .or_default() += 1;
        }
        counts
    }

    #[test]
    fn concurrent_appends_merge_with_every_event_once() {
        let m = machines();
        let url = m.remote.to_str().unwrap();
        let shared = note(&m.a.0, "before sync");
        m.a.1.init(Some(url)).unwrap();
        // b already has its own general ledger: unrelated histories union.
        let b_only = note(&m.b.0, "b before init");
        let outcome = m.b.1.init(Some(url)).unwrap();
        assert_eq!(outcome.error, None);

        let a1 = note(&m.a.0, "a concurrent");
        let b1 = note(&m.b.0, "b concurrent");
        assert_eq!(m.a.1.sync().unwrap().error, None);
        assert_eq!(m.b.1.sync().unwrap().error, None);
        assert_eq!(m.a.1.sync().unwrap().error, None);

        for store in [&m.a.0, &m.b.0] {
            let counts = event_counts(store);
            assert_eq!(counts.len(), 4, "{counts:?}");
            for id in [&shared, &b_only, &a1, &b1] {
                assert_eq!(counts.get(id), Some(&1), "{id} in {counts:?}");
            }
            assert!(store.general().replay().unwrap().issues.is_empty());
        }
        let status = m.a.1.status().unwrap();
        assert_eq!(
            (status.pending_commits, status.behind, status.uncommitted),
            (0, 0, false)
        );
        assert_eq!(
            fs::read_to_string(m.a.0.root().join(".gitattributes")).unwrap(),
            GITATTRIBUTES
        );
    }

    #[test]
    fn offline_push_keeps_local_writes_pending() {
        let m = machines();
        m.a.1.init(Some(m.remote.to_str().unwrap())).unwrap();
        fs::rename(&m.remote, m.remote.with_extension("gone")).unwrap();
        note(&m.a.0, "written offline");
        let outcome = m.a.1.sync().unwrap();
        assert!(outcome.error.is_some());
        assert!(!outcome.pushed);
        let status = m.a.1.status().unwrap();
        assert_eq!(status.pending_commits, 1);
        assert!(status.state.last_error.is_some());
        assert_eq!(m.a.0.general().active_memories().unwrap().len(), 1);

        fs::rename(m.remote.with_extension("gone"), &m.remote).unwrap();
        let outcome = m.a.1.sync().unwrap();
        assert_eq!(outcome.error, None);
        let status = m.a.1.status().unwrap();
        assert_eq!((status.pending_commits, status.state.last_error), (0, None));
    }

    #[test]
    fn rejected_push_is_retried_after_pull() {
        let m = machines();
        let url = m.remote.to_str().unwrap();
        m.a.1.init(Some(url)).unwrap();
        m.b.1.init(Some(url)).unwrap();
        note(&m.a.0, "a first");
        assert!(m.a.1.push().unwrap().pushed);
        note(&m.b.0, "b behind");
        // b has not pulled: its push is rejected, then pulls and retries.
        let outcome = m.b.1.push().unwrap();
        assert_eq!(outcome.error, None);
        assert!(outcome.pushed);
        assert_eq!(event_counts(&m.b.0).len(), 2);
        m.a.1.pull().unwrap();
        assert_eq!(event_counts(&m.a.0).len(), 2);
    }

    #[test]
    fn local_state_is_not_synced() {
        let m = machines();
        m.a.1.init(Some(m.remote.to_str().unwrap())).unwrap();
        let repo = m.a.0.root().join("local");
        assert!(repo.join(STATE_FILE).exists());
        let tracked = m.a.1.git_ok(&["ls-files"]).unwrap();
        assert!(!tracked.contains("local/"), "{tracked}");
    }

    #[test]
    fn machine_local_files_committed_by_old_versions_heal() {
        let m = machines();
        let url = m.remote.to_str().unwrap();
        // Both machines committed their own service.pid (old .gitignore).
        for (store, sync, pid) in [(&m.a.0, &m.a.1, "111"), (&m.b.0, &m.b.1, "222")] {
            note(store, "x");
            sync.git_ok(&["init", "-q", "-b", BRANCH]).unwrap();
            sync.git_ok(&["remote", "add", REMOTE, url]).unwrap();
            fs::write(store.root().join("service.pid"), pid).unwrap();
            fs::write(store.root().join(".gitignore"), "/local/\n").unwrap();
            sync.git_ok(&["add", "-A"]).unwrap();
            sync.commit_staged("old version").unwrap();
        }
        m.a.1.git_ok(&["push", "-q", "-u", REMOTE, BRANCH]).unwrap();
        fs::create_dir_all(m.a.0.root().join("stores")).unwrap();
        fs::write(m.a.0.root().join("stores/legacy.db"), "x").unwrap();

        // b: add/add conflict on service.pid is resolved, not fatal.
        assert_eq!(m.b.1.sync().unwrap().error, None);
        assert_eq!(m.a.1.sync().unwrap().error, None);
        for (store, sync, pid) in [(&m.a.0, &m.a.1, "111"), (&m.b.0, &m.b.1, "222")] {
            let tracked = sync.git_ok(&["ls-files"]).unwrap();
            assert!(tracked.lines().all(synced), "{tracked}");
            assert_eq!(
                fs::read_to_string(store.root().join("service.pid")).unwrap(),
                pid
            );
            assert_eq!(event_counts(store).len(), 2);
            let status = sync.status().unwrap();
            assert_eq!(
                (status.pending_commits, status.behind, status.uncommitted),
                (0, 0, false)
            );
        }
        assert!(m.a.0.root().join("stores/legacy.db").exists());
    }

    #[test]
    fn ledger_deleted_here_but_appended_there_is_kept() {
        let m = machines();
        let url = m.remote.to_str().unwrap();
        let dup = |store: &Store| {
            let dir = store.root().join("repos/oqto--abc");
            fs::create_dir_all(&dir).unwrap();
            MemoryFile::new(dir.join("mmry.jsonl"))
        };
        let first = MemoryEvent::add(
            "x".into(),
            MemoryType::Semantic,
            Vec::new(),
            &AgentCtx::default(),
        );
        dup(&m.a.0).append(&first).unwrap();
        m.a.1.init(Some(url)).unwrap();
        m.b.1.init(Some(url)).unwrap();
        // a merged the duplicate away; b, not yet pulled, appended to it.
        fs::remove_dir_all(m.a.0.root().join("repos/oqto--abc")).unwrap();
        assert_eq!(m.a.1.sync().unwrap().error, None);
        let late = MemoryEvent::add(
            "late".into(),
            MemoryType::Semantic,
            Vec::new(),
            &AgentCtx::default(),
        );
        dup(&m.b.0).append(&late).unwrap();

        assert_eq!(m.b.1.sync().unwrap().error, None);
        assert_eq!(m.a.1.sync().unwrap().error, None);
        for store in [&m.a.0, &m.b.0] {
            let mut ids: Vec<_> = dup(store)
                .replay()
                .unwrap()
                .entries
                .into_iter()
                .map(|e| e.memory_id)
                .collect();
            ids.sort();
            let mut expected = vec![first.memory_id.clone(), late.memory_id.clone()];
            expected.sort();
            assert_eq!(ids, expected);
        }
    }

    #[test]
    fn synced_paths() {
        for path in [
            ".gitignore",
            ".gitattributes",
            "general/mmry.jsonl",
            "repos/a--1/repo.json",
        ] {
            assert!(synced(path), "{path}");
        }
        for path in [
            "service.pid",
            "local/sync.json",
            "stores/x.db",
            "repos/a--1/mmry.jsonl.tmp-9",
            "repo.tmp",
        ] {
            assert!(!synced(path), "{path}");
        }
    }

    #[test]
    fn commands_need_init() {
        let m = machines();
        assert!(!m.a.1.is_enabled());
        assert!(
            m.a.1
                .commit()
                .unwrap_err()
                .to_string()
                .contains("mmry sync init")
        );
        assert!(!m.a.1.status().unwrap().enabled);
    }
}
