//! Bounded discovery and in-memory aggregation of repository ledgers.

use crate::config::DiscoveryRoot;
use crate::MemoryEntry;
use crate::MemoryFile;
use crate::ScoredMemory;
use rayon::prelude::*;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;
use walkdir::DirEntry;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Repository {
    pub name: String,
    pub repo_path: PathBuf,
    pub memory_path: PathBuf,
    pub exists: bool,
    pub readable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepositoryMemory {
    pub repo: String,
    pub repo_path: PathBuf,
    #[serde(flatten)]
    pub memory: MemoryEntry,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepositorySearchHit {
    pub repo: String,
    pub repo_path: PathBuf,
    pub score: usize,
    #[serde(flatten)]
    pub memory: MemoryEntry,
}

pub fn discover(roots: &[DiscoveryRoot]) -> crate::Result<Vec<Repository>> {
    let mut repos = Vec::new();
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
            if !entry.file_type().is_dir() {
                continue;
            }
            let memory_path = entry.path().join(".mmry/mmry.jsonl");
            if memory_path.exists() {
                let repo_path = std::fs::canonicalize(entry.path())?;
                let readable = std::fs::File::open(&memory_path).is_ok();
                repos.push(Repository {
                    name: repo_path
                        .file_name()
                        .unwrap_or(repo_path.as_os_str())
                        .to_string_lossy()
                        .into_owned(),
                    repo_path,
                    memory_path,
                    exists: true,
                    readable,
                });
            }
        }
    }
    repos.sort_by(|a, b| a.repo_path.cmp(&b.repo_path));
    repos.dedup_by(|a, b| a.repo_path == b.repo_path);
    Ok(repos)
}

fn include_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !matches!(
        name.as_ref(),
        ".git" | "target" | "node_modules" | ".cache" | ".cargo" | ".npm" | ".pnpm-store"
    )
}

pub fn select_named(repos: &[Repository], name: &str) -> crate::Result<Repository> {
    let matches: Vec<_> = repos
        .iter()
        .filter(|repo| repo.name == name)
        .cloned()
        .collect();
    match matches.as_slice() {
        [] => Err(crate::Error::NotFound(format!("repository '{name}'"))),
        [repo] => Ok(repo.clone()),
        _ => Err(crate::Error::InvalidInput(format!(
            "repository name '{name}' is ambiguous:\n{}",
            matches
                .iter()
                .map(|repo| format!("  {}", repo.repo_path.display()))
                .collect::<Vec<_>>()
                .join("\n")
        ))),
    }
}

pub fn list(repos: &[Repository]) -> crate::Result<Vec<RepositoryMemory>> {
    let results: Vec<crate::Result<Vec<_>>> = repos
        .par_iter()
        .map(|repo| {
            MemoryFile::open_at(&repo.repo_path)
                .active_memories()
                .map(|memories| {
                    memories
                        .into_iter()
                        .map(|memory| RepositoryMemory {
                            repo: repo.name.clone(),
                            repo_path: repo.repo_path.clone(),
                            memory,
                        })
                        .collect()
                })
        })
        .collect();
    let mut merged = results
        .into_iter()
        .collect::<crate::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    merged.sort_by(|a, b| {
        b.memory
            .updated_at
            .cmp(&a.memory.updated_at)
            .then_with(|| a.repo_path.cmp(&b.repo_path))
            .then_with(|| a.memory.memory_id.cmp(&b.memory.memory_id))
    });
    Ok(merged)
}

pub fn search(
    repos: &[Repository],
    query: &str,
    limit: usize,
) -> crate::Result<Vec<RepositorySearchHit>> {
    let results: Vec<crate::Result<Vec<ScoredMemory>>> = repos
        .par_iter()
        .map(|repo| MemoryFile::open_at(&repo.repo_path).search(query, usize::MAX))
        .collect();
    let mut merged = Vec::new();
    for (repo, hits) in repos.iter().zip(results) {
        for hit in hits? {
            merged.push(RepositorySearchHit {
                repo: repo.name.clone(),
                repo_path: repo.repo_path.clone(),
                score: hit.score,
                memory: hit.memory,
            });
        }
    }
    merged.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.memory.updated_at.cmp(&a.memory.updated_at))
            .then_with(|| a.repo_path.cmp(&b.repo_path))
            .then_with(|| a.memory.memory_id.cmp(&b.memory.memory_id))
    });
    merged.truncate(limit);
    Ok(merged)
}

pub fn repository_for_path(path: &Path) -> crate::Result<Repository> {
    let path = std::fs::canonicalize(path)?;
    let memory_path = path.join(".mmry/mmry.jsonl");
    Ok(Repository {
        name: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        repo_path: path,
        exists: memory_path.exists(),
        readable: std::fs::File::open(&memory_path).is_ok(),
        memory_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentCtx;
    use crate::MemoryEvent;
    use crate::MemoryType;
    use std::fs;

    fn repo(root: &Path, name: &str, text: &str) {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        MemoryFile::open_at(&path)
            .append(&MemoryEvent::add(
                text.into(),
                MemoryType::Semantic,
                vec![],
                &AgentCtx::default(),
            ))
            .unwrap();
    }

    #[test]
    fn discovers_excludes_and_aggregates() {
        let dir = tempfile::tempdir().unwrap();
        repo(dir.path(), "a", "release alpha");
        repo(dir.path(), "b", "release beta");
        repo(&dir.path().join("target"), "ignored", "release");
        let roots = [DiscoveryRoot {
            path: dir.path().into(),
            max_depth: 2,
        }];
        let repos = discover(&roots).unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(search(&repos, "release", 10).unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlink_loops() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        repo(dir.path(), "a", "x");
        symlink(dir.path(), dir.path().join("a/loop")).unwrap();
        assert_eq!(
            discover(&[DiscoveryRoot {
                path: dir.path().into(),
                max_depth: 8
            }])
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn named_selection_handles_success_zero_and_ambiguity() {
        let dir = tempfile::tempdir().unwrap();
        repo(&dir.path().join("one"), "same", "x");
        repo(&dir.path().join("two"), "same", "y");
        repo(dir.path(), "unique", "z");
        let repos = discover(&[DiscoveryRoot {
            path: dir.path().into(),
            max_depth: 3,
        }])
        .unwrap();
        assert_eq!(select_named(&repos, "unique").unwrap().name, "unique");
        assert!(select_named(&repos, "missing")
            .unwrap_err()
            .to_string()
            .contains("not found"));
        let error = select_named(&repos, "same").unwrap_err().to_string();
        assert!(error.contains("one/same"));
        assert!(error.contains("two/same"));
    }

    #[test]
    fn aggregation_order_has_stable_repository_tie_break() {
        let dir = tempfile::tempdir().unwrap();
        repo(dir.path(), "b", "same");
        repo(dir.path(), "a", "same");
        let repos = discover(&[DiscoveryRoot {
            path: dir.path().into(),
            max_depth: 1,
        }])
        .unwrap();
        let first = list(&repos).unwrap();
        let second = list(&repos).unwrap();
        assert_eq!(
            first.iter().map(|item| &item.repo_path).collect::<Vec<_>>(),
            second
                .iter()
                .map(|item| &item.repo_path)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn five_hundred_repository_fixture_completes() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..500 {
            repo(dir.path(), &format!("r{index}"), "small ledger");
        }
        let start = std::time::Instant::now();
        let repos = discover(&[DiscoveryRoot {
            path: dir.path().into(),
            max_depth: 1,
        }])
        .unwrap();
        let memories = list(&repos).unwrap();
        eprintln!("500 repository cold fixture: {:?}", start.elapsed());
        assert_eq!(memories.len(), 500);
    }
}
