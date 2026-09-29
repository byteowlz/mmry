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

    /// The single central ledger whose directory starts with `name--`.
    fn central(&self, name: &str) -> PathBuf {
        let dirs: Vec<_> = fs::read_dir(self.state.join("repos"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&format!("{name}--"))
            })
            .collect();
        assert_eq!(dirs.len(), 1, "{dirs:?}");
        dirs[0].join("mmry.jsonl")
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
    assert!(sb.central("app").exists());
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
    assert!(repo_item["repo"].as_str().unwrap().starts_with("app--"));
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
fn machine_dot_uses_agent_ctx_machine_id_and_provenance_is_recorded() {
    let sb = Sandbox::new("");
    let app = sb.repo("app");
    let output = Command::new(env!("CARGO_BIN_EXE_mmry"))
        .current_dir(&app)
        .args(["add", "needs --no-sandbox", "--machine", ".", "--json"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &sb.home)
        .env("MMRY_CONFIG", &sb.config)
        .env("AGENT_CTX_MACHINE_ID", "m-42")
        .env("AGENT_CTX_AGENT_ID", "agent-7")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let listed = sb.json(&app, &["list", "--json"]);
    assert_eq!(listed[0]["machine"], "m-42");
    let ledger = fs::read_to_string(sb.central("app")).unwrap();
    assert!(ledger.contains("\"agent_id\":\"agent-7\""), "{ledger}");
}

#[test]
fn doctor_reports_contested_memories_and_damage() {
    let sb = Sandbox::new("");
    let app = sb.repo("app");
    let added = sb.json(&app, &["add", "v1", "--json"]);
    let id = added["memory_id"].as_str().unwrap();
    sb.ok(&app, &["supersede", id, "v2 here", "--reason", "r"]);
    assert!(
        sb.json(&app, &["doctor", "--json"])["healthy"]
            .as_bool()
            .unwrap()
    );

    // Another machine superseded revision 1 concurrently; the sync merged it.
    let ledger = sb.central("app");
    let text = fs::read_to_string(&ledger).unwrap();
    let mut other: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    other["id"] = "evt_other_machine".into();
    other["content"] = "v2 there".into();
    // Same timestamp; the id sorts after the local edit ("evt_o" > "evt_<hex>").
    fs::write(&ledger, format!("{text}{other}\n{{\"truncated\n")).unwrap();

    let doctor = sb.json(&app, &["doctor", "--json"]);
    assert_eq!(doctor["healthy"], false);
    let repo = &doctor["ledgers"][1];
    assert_eq!(repo["contested"], serde_json::json!([id]));
    assert_eq!(repo["issues"][0]["kind"], "malformed");
    assert!(!sb.run(&app, &["doctor"]).status.success());

    let listed = sb.json(&app, &["list", "--json"]);
    assert_eq!(
        (
            listed[0]["contested"].as_bool(),
            listed[0]["content"].as_str()
        ),
        (Some(true), Some("v2 there"))
    );
    assert!(sb.ok(&app, &["list"]).contains("CONTESTED"));

    sb.ok(&app, &["supersede", id, "v3 agreed", "--reason", "merge"]);
    let listed = sb.json(&app, &["list", "--json"]);
    assert_eq!(listed[0]["contested"], false);
}

#[test]
fn preview_contract_and_entry_json_for_writes() {
    let sb = Sandbox::new("");
    let app = sb.repo("app");
    let added = sb.json(&app, &["add", "repo fact", "--why", "because", "--json"]);
    assert_eq!(
        (
            added["scope"].as_str(),
            added["revision"].as_u64(),
            added["why"].as_str()
        ),
        (Some("repo"), Some(1), Some("because"))
    );
    let id = added["memory_id"].as_str().unwrap();
    sb.ok(&app, &["add", "--general", "general fact"]);
    let superseded = sb.json(
        &app,
        &["supersede", id, "repo fact v2", "--reason", "r", "--json"],
    );
    assert_eq!(
        (
            superseded["content"].as_str(),
            superseded["revision"].as_u64()
        ),
        (Some("repo fact v2"), Some(2))
    );

    // --cwd selects the repository independent of the working directory.
    let preview = sb.json(
        &sb.home,
        &["preview", "--json", "--cwd", app.to_str().unwrap()],
    );
    assert_eq!(preview["schema_version"], 1);
    let origins: Vec<_> = preview["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["origin"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(origins, ["app", "general"]);
    let rendered = preview["rendered"].as_str().unwrap();
    assert!(
        rendered.starts_with("<mmry>\n") && rendered.contains("[repo app]"),
        "{rendered}"
    );
    assert_eq!(
        sb.ok(&app, &["preview"]),
        rendered,
        "plain output is exactly `rendered`"
    );
    let again = sb.json(&app, &["preview", "--json"]);
    assert_eq!(again["selection_hash"], preview["selection_hash"]);

    let small = sb.json(&app, &["preview", "--json", "--limit", "1"]);
    assert_eq!(
        (
            small["entries"].as_array().unwrap().len(),
            small["omitted"].as_u64()
        ),
        (1, Some(1))
    );

    let removed = sb.json(&app, &["rm", id, "--json"]);
    assert_eq!(
        (removed["memory_id"].as_str(), removed["removed"].as_bool()),
        (Some(id), Some(true))
    );
    let after = sb.json(&app, &["preview", "--json"]);
    assert_ne!(after["selection_hash"], preview["selection_hash"]);
}

#[test]
fn preview_never_prompts_or_migrates_and_warns_about_pending_ledgers() {
    let sb = Sandbox::new("migrate = 'auto'\n");
    let app = sb.repo("app");
    seed_local_ledger(&sb, &app, "legacy memory");
    let preview = sb.json(&app, &["preview", "--json"]);
    assert!(app.join(".mmry/mmry.jsonl").exists());
    assert!(
        preview["warnings"][0]
            .as_str()
            .unwrap()
            .contains("mmry setup")
    );
    assert_eq!(preview["rendered"], "");
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
    let sb = Sandbox::new("migrate = 'auto'\n");
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
    let central = fs::read_to_string(sb.central("app")).unwrap();
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
fn default_prompt_without_terminal_warns_and_continues() {
    let sb = Sandbox::new("");
    let app = sb.repo("app");
    seed_local_ledger(&sb, &app, "legacy memory");
    let output = sb.run(&app, &["list", "--json"]);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("mmry setup"));
    assert!(app.join(".mmry/mmry.jsonl").exists());
    // Writes still work and go to the central store only.
    sb.ok(&app, &["add", "new memory"]);
    assert!(
        !fs::read_to_string(app.join(".mmry/mmry.jsonl"))
            .unwrap()
            .contains("new memory")
    );
    assert!(
        !fs::read_to_string(&sb.config)
            .unwrap()
            .contains("migrate = ")
    );
}

#[test]
fn setup_plans_asks_and_migrates_everything() {
    let sb = Sandbox::new("");
    let one = sb.repo("one");
    let two = sb.repo("nested/deeper/two");
    seed_local_ledger(&sb, &one, "first");
    seed_local_ledger(&sb, &two, "second");

    let plan = sb.json(&sb.home, &["setup", "--dry-run", "--json"]);
    let statuses: Vec<_> = plan["reports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["status"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(statuses, ["migrated", "migrated"]);
    assert!(one.join(".mmry/mmry.jsonl").exists() && !sb.state.join("repos").exists());

    let refused = sb.run(&sb.home, &["setup"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--yes"));
    assert!(one.join(".mmry/mmry.jsonl").exists());

    sb.ok(&sb.home, &["setup", "--yes"]);
    assert!(!one.join(".mmry/mmry.jsonl").exists() && !two.join(".mmry/mmry.jsonl").exists());
    let config = fs::read_to_string(&sb.config).unwrap();
    assert!(
        config.contains("state_root = ") && config.contains("migrate = \"auto\""),
        "{config}"
    );
    assert_eq!(
        contents(&sb.json(&two, &["list", "--json"])),
        [pair("repo", "second")]
    );
    // Idempotent.
    let again = sb.json(&sb.home, &["setup", "--yes", "--json"]);
    assert_eq!(again["reports"], serde_json::json!([]));
}

#[test]
fn flags_override_env_override_config() {
    let sb = Sandbox::new("");
    let app = sb.repo("app");
    let env_root = sb.home.join("env-state");
    let flag_root = sb.home.join("flag-state");
    let run = |args: &[&str], env_value: &Path| {
        let output = Command::new(env!("CARGO_BIN_EXE_mmry"))
            .current_dir(&app)
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &sb.home)
            .env("MMRY_CONFIG", &sb.config)
            .env("MMRY_STATE_ROOT", env_value)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    run(&["add", "--general", "via env"], &env_root);
    assert!(env_root.join("general/mmry.jsonl").exists());
    let flag = flag_root.to_str().unwrap();
    run(
        &["--state-root", flag, "add", "--general", "via flag"],
        &env_root,
    );
    assert!(flag_root.join("general/mmry.jsonl").exists());
    assert!(!sb.state.join("general").exists());

    seed_local_ledger(&sb, &app, "legacy");
    let output = sb.run(&app, &["--migrate", "auto", "list", "--json"]);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("migrated"),
        "{output:?}"
    );
    assert!(
        !sb.run(&app, &["--migrate", "never", "list"])
            .status
            .success()
    );
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
