//! End-to-end tests of the `mmry` binary with an isolated state root.

#![cfg(test)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

struct Sandbox {
    _dir: tempfile::TempDir,
    home: PathBuf,
    state: PathBuf,
    config: PathBuf,
}

impl Sandbox {
    fn new(extra_config: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let state = dir.path().join("state");
        let config = dir.path().join("config.toml");
        fs::create_dir_all(&home).unwrap();
        fs::write(
            &config,
            format!("state_root = '{}'\n{extra_config}", state.display()),
        )
        .unwrap();
        Self {
            home,
            state,
            config,
            _dir: dir,
        }
    }

    fn repo(&self, name: &str) -> PathBuf {
        let path = self.home.join(name);
        fs::create_dir_all(&path).unwrap();
        let git = |args: &[&str]| {
            let ok = Command::new("git")
                .arg("-C")
                .arg(&path)
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
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q"]);
        fs::write(path.join("README"), name).unwrap();
        git(&["add", "README"]);
        git(&["commit", "-q", "-m", "init"]);
        path
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_mmry"))
            .current_dir(cwd)
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("MMRY_CONFIG", &self.config)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, cwd: &Path, args: &[&str]) -> String {
        let output = self.run(cwd, args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    fn json(&self, cwd: &Path, args: &[&str]) -> serde_json::Value {
        serde_json::from_str(&self.ok(cwd, args)).unwrap()
    }
}

fn contents(items: &serde_json::Value) -> Vec<(String, String)> {
    let mut pairs: Vec<_> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["scope"].as_str().unwrap().to_owned(),
                item["content"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    pairs.sort();
    pairs
}

fn pair(scope: &str, content: &str) -> (String, String) {
    (scope.to_owned(), content.to_owned())
}

#[test]
fn repo_and_general_memories_are_scoped_and_labelled() {
    let sb = Sandbox::new("");
    let app = sb.repo("app");
    let other = sb.repo("other");
    sb.ok(&app, &["add", "app gotcha", "--why", "because"]);
    sb.ok(&app, &["add", "--general", "personal gotcha"]);
    sb.ok(&other, &["add", "other gotcha"]);

    assert!(
        !app.join(".mmry").exists(),
        "central mode must not write repo-local files"
    );
    assert!(sb.state.join("repos/app/mmry.jsonl").exists());
    assert!(sb.state.join("general/mmry.jsonl").exists());

    let in_app = sb.json(&app, &["list", "--json"]);
    assert_eq!(
        contents(&in_app),
        [
            pair("general", "personal gotcha"),
            pair("repo", "app gotcha")
        ]
    );
    let repo_item = in_app
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["scope"] == "repo")
        .unwrap();
    assert_eq!(repo_item["repo"], "app");
    assert_eq!(repo_item["why"], "because");
    assert!(
        repo_item["recorded_scope"]
            .as_str()
            .unwrap()
            .starts_with("repo:git:")
    );
    // No duplicate keys from flattening.
    let raw = sb.ok(&app, &["list", "--json"]);
    assert_eq!(raw.matches("\"scope\"").count(), 2);

    let general_only = sb.json(&app, &["list", "--general", "--json"]);
    assert_eq!(
        contents(&general_only),
        [pair("general", "personal gotcha")]
    );
    let other_repo = sb.json(&app, &["search", "gotcha", "--repo", "other", "--json"]);
    assert_eq!(contents(&other_repo), [pair("repo", "other gotcha")]);
    assert_eq!(
        sb.json(&app, &["list", "--all", "--json"])
            .as_array()
            .unwrap()
            .len(),
        3
    );

    // Outside any repository only general memories are in scope.
    assert_eq!(
        contents(&sb.json(&sb.home, &["list", "--json"])),
        [pair("general", "personal gotcha")]
    );
    let outside = sb.run(&sb.home, &["add", "needs a repo"]);
    assert!(!outside.status.success());
    assert!(String::from_utf8_lossy(&outside.stderr).contains("--general"));
}

#[test]
fn supersede_checks_revision_and_rm_deprecates() {
    let sb = Sandbox::new("");
    let app = sb.repo("app");
    let added = sb.json(&app, &["add", "use -x", "--json"]);
    let id = added["memory_id"].as_str().unwrap();

    sb.ok(
        &app,
        &[
            "supersede",
            id,
            "use -y",
            "--reason=-x removed",
            "--expected-revision",
            "1",
        ],
    );
    let stale = sb.run(
        &app,
        &[
            "supersede",
            id,
            "use -z",
            "--reason",
            "late",
            "--expected-revision",
            "1",
        ],
    );
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("revision 2, expected 1"));

    let listed = sb.json(&app, &["list", "--json"]);
    assert_eq!(listed[0]["content"], "use -y");
    assert_eq!(listed[0]["revision"], 2);
    assert_eq!(listed[0]["memory_id"], id);

    sb.ok(
        &app,
        &["rm", id, "--reason", "obsolete", "--expected-revision", "2"],
    );
    assert_eq!(sb.json(&app, &["list", "--json"]), serde_json::json!([]));
    let missing = sb.run(&app, &["rm", id]);
    assert!(!missing.status.success());
}

#[test]
fn expired_memories_are_hidden_unless_requested() {
    let sb = Sandbox::new("");
    let app = sb.repo("app");
    sb.ok(&app, &["add", "old", "--expires", "2000-01-01T00:00:00Z"]);
    sb.ok(&app, &["add", "new", "--expires", "30d"]);
    assert_eq!(
        contents(&sb.json(&app, &["list", "--json"])),
        [pair("repo", "new")]
    );
    assert_eq!(
        sb.json(&app, &["list", "--json", "--include-expired"])
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        !sb.run(&app, &["add", "x", "--expires", "soon"])
            .status
            .success()
    );
}

fn seed_local_ledger(sb: &Sandbox, repo: &Path, text: &str) {
    // Write a legacy repo-local ledger through tracked mode, then drop the marker.
    let init = sb.run(repo, &["init", "--tracked"]);
    assert!(init.status.success(), "{init:?}");
    sb.ok(repo, &["add", text]);
    fs::remove_file(repo.join(".mmry/tracked")).unwrap();
}

#[test]
fn auto_migration_moves_local_ledger_on_first_use() {
    let sb = Sandbox::new("");
    let app = sb.repo("app");
    seed_local_ledger(&sb, &app, "legacy memory");

    let output = sb.run(&app, &["list", "--json"]);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("migrated"));
    let listed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(contents(&listed), [pair("repo", "legacy memory")]);
    assert!(!app.join(".mmry/mmry.jsonl").exists());
    assert!(app.join(".mmry/MIGRATED").exists());

    // Writes now go to the central store only.
    sb.ok(&app, &["add", "after migration"]);
    assert!(!app.join(".mmry/mmry.jsonl").exists());
    let central = fs::read_to_string(sb.state.join("repos/app/mmry.jsonl")).unwrap();
    assert!(central.contains("after migration") && central.contains("legacy memory"));
}

#[test]
fn migrate_off_warns_and_never_reads_local() {
    let sb = Sandbox::new("migrate = 'off'\n");
    let app = sb.repo("app");
    seed_local_ledger(&sb, &app, "legacy memory");
    let output = sb.run(&app, &["list", "--json"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("mmry migrate"));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!([])
    );
    assert!(app.join(".mmry/mmry.jsonl").exists());

    let dry = sb.json(&app, &["migrate", "--dry-run", "--json"]);
    assert_eq!(dry[0]["status"], "migrated");
    assert_eq!(dry[0]["dry_run"], true);
    assert!(app.join(".mmry/mmry.jsonl").exists());
    sb.ok(&app, &["migrate"]);
    assert_eq!(
        contents(&sb.json(&app, &["list", "--json"])),
        [pair("repo", "legacy memory")]
    );
}

#[test]
fn migrate_prompt_without_terminal_fails_with_hint() {
    let sb = Sandbox::new("migrate = 'prompt'\n");
    let app = sb.repo("app");
    seed_local_ledger(&sb, &app, "legacy memory");
    let output = sb.run(&app, &["list"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("mmry migrate"));
    assert!(app.join(".mmry/mmry.jsonl").exists());
}

#[test]
fn tracked_mode_stays_repo_local() {
    let sb = Sandbox::new("");
    let app = sb.repo("app");
    sb.ok(&app, &["init", "--tracked"]);
    sb.ok(&app, &["add", "shared with the repo"]);
    assert!(
        fs::read_to_string(app.join(".mmry/mmry.jsonl"))
            .unwrap()
            .contains("shared with the repo")
    );
    assert!(!sb.state.join("repos").exists());
    let doctor = sb.ok(&app, &["doctor"]);
    assert!(doctor.contains("mode: tracked"), "{doctor}");
}

#[test]
fn explicit_missing_config_fails() {
    let sb = Sandbox::new("");
    let output = sb.run(&sb.home, &["--config", "/nonexistent/mmry.toml", "list"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("/nonexistent/mmry.toml"));
}
