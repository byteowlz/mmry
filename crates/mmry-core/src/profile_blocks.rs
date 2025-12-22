use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::agents::AgentEvent;
use crate::agents::AgentRecord;
use crate::config::Config;
use crate::database::operations;
use crate::Result;

pub const RESERVED_BLOCK_PERSONA: &str = "persona";
pub const RESERVED_BLOCK_HUMAN: &str = "human";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ProfileBlockScope {
    Global,
    Project,
    Agent,
}

impl ProfileBlockScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
            Self::Agent => "agent",
        }
    }
}

impl std::str::FromStr for ProfileBlockScope {
    type Err = crate::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "global" => Ok(Self::Global),
            "project" => Ok(Self::Project),
            "agent" => Ok(Self::Agent),
            other => Err(crate::Error::InvalidInput(format!(
                "Invalid profile block scope '{other}' (expected: global, project, agent)"
            ))),
        }
    }
}

fn default_profile_block_scope() -> ProfileBlockScope {
    ProfileBlockScope::Project
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileBlock {
    pub name: String,
    #[serde(default = "default_profile_block_scope")]
    pub scope: ProfileBlockScope,
    pub content: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ProfileBlockPatchOp {
    Insert {
        /// 1-based line number to insert before (1 inserts at start, len+1 appends)
        before_line: usize,
        text: String,
    },
    Replace {
        /// 1-based inclusive start line
        start_line: usize,
        /// 1-based inclusive end line (must be >= start_line)
        end_line: usize,
        text: String,
    },
}

#[derive(Debug, Clone)]
pub struct ProfileBlocksService {
    max_block_chars: usize,
    ingestr_enabled: bool,
    ingestr_bin: PathBuf,
    ingestr_output_dir: Option<PathBuf>,
    ingestr_timeout_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct ProfileBlockWriteContext {
    pub scope: ProfileBlockScope,
    pub actor_id: Uuid,
    pub span_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileBlocksIngestOptions {
    pub scope: ProfileBlockScope,
    pub prefix: String,
    pub skip_hidden: bool,
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_blocks: usize,
    pub extensions: Vec<String>,
    pub dry_run: bool,
}

impl Default for ProfileBlocksIngestOptions {
    fn default() -> Self {
        Self {
            scope: ProfileBlockScope::Project,
            prefix: "ingest".to_string(),
            skip_hidden: true,
            max_files: 200,
            max_file_bytes: 64 * 1024,
            max_blocks: 16,
            extensions: vec![
                "md".to_string(),
                "txt".to_string(),
                "rs".to_string(),
                "toml".to_string(),
                "json".to_string(),
                "yaml".to_string(),
                "yml".to_string(),
            ],
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileBlocksIngestedBlock {
    pub name: String,
    pub scope: ProfileBlockScope,
    pub files: Vec<String>,
    pub chars: usize,
    pub written: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileBlocksIngestResult {
    pub root: String,
    pub scope: ProfileBlockScope,
    pub files_seen: usize,
    pub files_included: usize,
    pub bytes_read: u64,
    pub blocks: Vec<ProfileBlocksIngestedBlock>,
}

impl ProfileBlocksService {
    pub fn from_config(config: &Config) -> Self {
        Self {
            max_block_chars: config.profile_blocks.max_block_chars,
            ingestr_enabled: config.profile_blocks.ingestr_enabled,
            ingestr_bin: config.profile_blocks.ingestr_bin.clone(),
            ingestr_output_dir: config.profile_blocks.ingestr_output_dir.clone(),
            ingestr_timeout_seconds: config.profile_blocks.ingestr_timeout_seconds,
        }
    }

    pub async fn list_blocks(
        &self,
        pool: &SqlitePool,
        owner_id: Uuid,
    ) -> Result<Vec<ProfileBlock>> {
        let profile = operations::get_user_profile(pool, owner_id)
            .await?
            .unwrap_or_else(|| mk_empty_profile(owner_id));

        Ok(extract_blocks(&profile.profile))
    }

    pub async fn get_block(
        &self,
        pool: &SqlitePool,
        owner_id: Uuid,
        name: &str,
        scope: ProfileBlockScope,
    ) -> Result<Option<ProfileBlock>> {
        let profile = operations::get_user_profile(pool, owner_id).await?;
        let Some(profile) = profile else {
            return Ok(None);
        };

        Ok(extract_blocks(&profile.profile)
            .into_iter()
            .find(|b| b.name == name && b.scope == scope))
    }

    pub async fn set_block(
        &self,
        pool: &SqlitePool,
        owner_id: Uuid,
        name: &str,
        content: String,
        ctx: ProfileBlockWriteContext,
    ) -> Result<ProfileBlock> {
        validate_block_name(name)?;
        self.validate_block_content(&content)?;

        let mut profile = operations::get_user_profile(pool, owner_id)
            .await?
            .unwrap_or_else(|| mk_empty_profile(owner_id));

        let now = Utc::now();
        upsert_block_in_profile(&mut profile.profile, name, ctx.scope, &content, now)?;
        profile.updated_at = now;
        operations::set_user_profile(pool, &profile).await?;

        record_profile_block_event(
            pool,
            ctx.actor_id,
            ctx.span_id.clone(),
            owner_id,
            serde_json::json!({
                "operation": "set",
                "owner_id": owner_id,
                "block": name,
                "scope": ctx.scope.as_str(),
                "chars": content.chars().count(),
            }),
        )
        .await?;

        Ok(ProfileBlock {
            name: name.to_string(),
            scope: ctx.scope,
            content,
            updated_at: now,
        })
    }

    pub async fn patch_block(
        &self,
        pool: &SqlitePool,
        owner_id: Uuid,
        name: &str,
        ops: Vec<ProfileBlockPatchOp>,
        ctx: ProfileBlockWriteContext,
    ) -> Result<ProfileBlock> {
        validate_block_name(name)?;

        let mut profile = operations::get_user_profile(pool, owner_id)
            .await?
            .unwrap_or_else(|| mk_empty_profile(owner_id));

        let existing = extract_blocks(&profile.profile)
            .into_iter()
            .find(|b| b.name == name && b.scope == ctx.scope)
            .map(|b| b.content)
            .unwrap_or_default();

        let updated = apply_patch(existing, &ops)?;
        self.validate_block_content(&updated)?;

        let now = Utc::now();
        upsert_block_in_profile(&mut profile.profile, name, ctx.scope, &updated, now)?;
        profile.updated_at = now;
        operations::set_user_profile(pool, &profile).await?;

        record_profile_block_event(
            pool,
            ctx.actor_id,
            ctx.span_id.clone(),
            owner_id,
            serde_json::json!({
                "operation": "patch",
                "owner_id": owner_id,
                "block": name,
                "scope": ctx.scope.as_str(),
                "chars": updated.chars().count(),
                "ops": ops,
            }),
        )
        .await?;

        Ok(ProfileBlock {
            name: name.to_string(),
            scope: ctx.scope,
            content: updated,
            updated_at: now,
        })
    }

    pub fn render_for_prompt(&self, blocks: &[ProfileBlock]) -> String {
        let mut sorted = blocks.to_vec();
        sorted.sort_by_key(prompt_sort_key);

        let mut out = String::new();
        for block in sorted {
            if block.content.trim().is_empty() {
                continue;
            }
            out.push_str(&format!("# {}\n", block.name));
            out.push_str(block.content.trim_end());
            out.push_str("\n\n");
        }
        out.trim_end().to_string()
    }

    fn validate_block_content(&self, content: &str) -> Result<()> {
        let chars = content.chars().count();
        if chars > self.max_block_chars {
            return Err(crate::Error::InvalidInput(format!(
                "Profile block exceeds max_block_chars ({} > {})",
                chars, self.max_block_chars
            )));
        }
        Ok(())
    }

    pub async fn ingest_directory(
        &self,
        pool: &SqlitePool,
        owner_id: Uuid,
        root: &Path,
        opts: ProfileBlocksIngestOptions,
        actor_id: Uuid,
        span_id: Option<String>,
    ) -> Result<ProfileBlocksIngestResult> {
        if !root.exists() {
            return Err(crate::Error::InvalidInput(format!(
                "Ingest root does not exist: {}",
                root.display()
            )));
        }
        if !root.is_dir() {
            return Err(crate::Error::InvalidInput(format!(
                "Ingest root is not a directory: {}",
                root.display()
            )));
        }

        let conversion = self.maybe_run_ingestr(root, &opts).await?;
        let ingest_root = conversion
            .as_ref()
            .map(|dir| dir.path.as_path())
            .unwrap_or(root);

        let max_block_chars = self.max_block_chars;
        let root_buf = ingest_root.to_path_buf();
        let opts_clone = opts.clone();
        let plan = tokio::task::spawn_blocking(move || {
            build_ingest_plan(&root_buf, &opts_clone, max_block_chars)
        })
        .await
        .map_err(|e| crate::Error::Service(format!("Ingest task failed: {e}")))??;

        let mut blocks = Vec::with_capacity(plan.blocks.len());
        let write_ctx = ProfileBlockWriteContext {
            scope: opts.scope,
            actor_id,
            span_id,
        };
        for planned in plan.blocks {
            let mut written = false;
            if !opts.dry_run {
                let existing = self
                    .get_block(pool, owner_id, &planned.name, opts.scope)
                    .await?;
                if existing.as_ref().map(|b| b.content.as_str()) != Some(planned.content.as_str()) {
                    let _ = self
                        .set_block(
                            pool,
                            owner_id,
                            &planned.name,
                            planned.content.clone(),
                            write_ctx.clone(),
                        )
                        .await?;
                    written = true;
                }
            }

            blocks.push(ProfileBlocksIngestedBlock {
                name: planned.name,
                scope: opts.scope,
                files: planned.files,
                chars: planned.content.chars().count(),
                written,
            });
        }

        Ok(ProfileBlocksIngestResult {
            root: plan.root,
            scope: opts.scope,
            files_seen: plan.files_seen,
            files_included: plan.files_included,
            bytes_read: plan.bytes_read,
            blocks,
        })
    }

    async fn maybe_run_ingestr(
        &self,
        root: &Path,
        opts: &ProfileBlocksIngestOptions,
    ) -> Result<Option<IngestrOutputDir>> {
        if !self.ingestr_enabled {
            return Ok(None);
        }
        if opts.dry_run {
            return Ok(None);
        }

        let out = IngestrOutputDir::new(root, self.ingestr_output_dir.as_deref())?;
        tokio::fs::create_dir_all(&out.path).await.map_err(|e| {
            crate::Error::Service(format!(
                "Failed to create ingestr output dir {}: {e}",
                out.path.display()
            ))
        })?;

        let mut cmd = tokio::process::Command::new(&self.ingestr_bin);
        cmd.arg("service")
            .arg("run")
            .arg("--once")
            .arg("--watch-dir")
            .arg(root)
            .arg("--output-dir")
            .arg(&out.path)
            .arg("--disable-index")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let timeout = std::time::Duration::from_secs(self.ingestr_timeout_seconds.max(1));
        let output = tokio::time::timeout(timeout, cmd.output())
            .await
            .map_err(|_| {
                crate::Error::Service(format!(
                    "ingestr timed out after {}s",
                    self.ingestr_timeout_seconds.max(1)
                ))
            })?
            .map_err(|e| {
                crate::Error::Service(format!(
                    "Failed to execute ingestr ({}): {e}",
                    self.ingestr_bin.display()
                ))
            })?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(crate::Error::Service(format!(
                "ingestr failed (exit={}):\nstdout:\n{}\n\nstderr:\n{}",
                output.status,
                truncate_for_error(&stdout, 4000),
                truncate_for_error(&stderr, 4000)
            )));
        }

        Ok(Some(out))
    }
}

#[derive(Debug)]
struct IngestPlannedBlock {
    name: String,
    content: String,
    files: Vec<String>,
}

#[derive(Debug)]
struct IngestPlan {
    root: String,
    files_seen: usize,
    files_included: usize,
    bytes_read: u64,
    blocks: Vec<IngestPlannedBlock>,
}

fn build_ingest_plan(
    root: &Path,
    opts: &ProfileBlocksIngestOptions,
    max_block_chars: usize,
) -> Result<IngestPlan> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root_display = root.display().to_string();

    let ext_allow: Vec<String> = opts
        .extensions
        .iter()
        .map(|s| s.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let mut files_seen = 0usize;
    let mut files_included = 0usize;
    let mut bytes_read = 0u64;

    let mut selected: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !should_skip_ingest_entry(entry, &root, opts))
        .filter_map(std::result::Result::ok)
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        files_seen += 1;

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if !ext_allow.is_empty() && !ext_allow.iter().any(|e| e == &ext) {
            continue;
        }

        selected.push(path.to_path_buf());
        if selected.len() >= opts.max_files.max(1) {
            break;
        }
    }

    selected.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));

    let prefix = sanitize_block_segment(&opts.prefix).unwrap_or_else(|| "ingest".to_string());
    let slug = root
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(sanitize_block_segment)
        .unwrap_or_else(|| "dir".to_string());

    let base_name = shorten_block_name(&format!("{prefix}.{slug}"), 64);
    let header = format!("Ingested from: {root_display}\n\n");

    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut current_files: Vec<String> = Vec::new();
    current.push_str(&header);

    for path in selected {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        let meta = std::fs::metadata(&path).ok();
        let file_size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let (mut content, was_truncated, bytes) = match read_text_file_limited(&path, opts) {
            Ok(result) => result,
            Err(_) => continue,
        };
        bytes_read += bytes;

        content = strip_yaml_frontmatter(&content).to_string();
        content = normalize_newlines(&content);

        let mut section = String::new();
        section.push_str("## ");
        section.push_str(&rel);
        section.push_str("\n\n");
        section.push_str(content.trim_end());
        section.push('\n');
        if was_truncated || file_size > opts.max_file_bytes.max(1) as u64 {
            section.push_str("\n[truncated]\n");
        }
        section.push('\n');

        if section.chars().count() > max_block_chars {
            section = section
                .chars()
                .take(max_block_chars.saturating_sub(32))
                .collect();
            section.push_str("\n\n[truncated]\n");
        }

        if current.chars().count() + section.chars().count() > max_block_chars {
            if !current_files.is_empty() {
                blocks.push((current, current_files));
                current = header.clone();
                current_files = Vec::new();
            }

            if blocks.len() >= opts.max_blocks.max(1) {
                break;
            }
        }

        current.push_str(&section);
        current_files.push(rel);
        files_included += 1;
    }

    if !current_files.is_empty() && blocks.len() < opts.max_blocks.max(1) {
        blocks.push((current, current_files));
    }

    let mut planned = Vec::new();
    for (idx, (content, files)) in blocks.into_iter().enumerate() {
        let name = if idx == 0 {
            base_name.clone()
        } else {
            shorten_block_name(&format!("{base_name}.p{}", idx + 1), 64)
        };
        planned.push(IngestPlannedBlock {
            name,
            content,
            files,
        });
    }

    Ok(IngestPlan {
        root: root_display,
        files_seen,
        files_included,
        bytes_read,
        blocks: planned,
    })
}

fn truncate_for_error(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = text.chars().take(max_chars).collect::<String>();
    out.push_str("\n…(truncated)…");
    out
}

struct IngestrOutputDir {
    path: PathBuf,
    cleanup: bool,
}

impl IngestrOutputDir {
    fn new(root: &Path, configured_base: Option<&Path>) -> Result<Self> {
        if let Some(base) = configured_base {
            let suffix = format!("{:x}", fnv1a_64(root.to_string_lossy().as_bytes()));
            return Ok(Self {
                path: base.join(format!("mmry-ingestr-{suffix}")),
                cleanup: false,
            });
        }

        Ok(Self {
            path: std::env::temp_dir().join(format!("mmry-ingestr-{}", Uuid::new_v4())),
            cleanup: true,
        })
    }
}

impl Drop for IngestrOutputDir {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn should_skip_ingest_entry(
    entry: &walkdir::DirEntry,
    _root: &Path,
    opts: &ProfileBlocksIngestOptions,
) -> bool {
    let name = entry.file_name().to_str().unwrap_or_default();

    if opts.skip_hidden && name.starts_with('.') {
        return true;
    }

    entry.file_type().is_dir() && matches!(name, ".git" | "target" | "node_modules" | ".beads")
}

fn strip_yaml_frontmatter(body: &str) -> &str {
    let Some(rest) = body.strip_prefix("---\n") else {
        return body;
    };
    let Some(idx) = rest.find("\n---\n") else {
        return body;
    };
    &rest[idx + "\n---\n".len()..]
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn sanitize_block_segment(segment: &str) -> Option<String> {
    let mut out = String::new();
    for c in segment.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c.to_ascii_lowercase());
        } else if c == '.' {
            out.push('_');
        }
    }
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn shorten_block_name(name: &str, max_len: usize) -> String {
    if name.len() <= max_len {
        return name.to_string();
    }

    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x00000100000001B3);
    }
    let suffix = format!(".{:x}", hash);
    let budget = max_len.saturating_sub(suffix.len()).max(1);
    let mut out = name.chars().take(budget).collect::<String>();
    out.push_str(&suffix);
    out
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x00000100000001B3);
    }
    hash
}

fn read_text_file_limited(
    path: &Path,
    opts: &ProfileBlocksIngestOptions,
) -> Result<(String, bool, u64)> {
    let meta = std::fs::metadata(path)?;
    let file_size = meta.len();
    let max_bytes = opts.max_file_bytes.max(1) as u64;

    let mut file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    let bytes_read = std::io::Read::by_ref(&mut file)
        .take(max_bytes)
        .read_to_end(&mut buf)? as u64;

    if buf.contains(&0) {
        return Err(crate::Error::InvalidInput(format!(
            "File appears binary: {}",
            path.display()
        )));
    }

    let (text, invalid) = match std::str::from_utf8(&buf) {
        Ok(s) => (s.to_string(), false),
        Err(e) => {
            let valid = e.valid_up_to();
            if valid == 0 {
                return Err(crate::Error::InvalidInput(format!(
                    "File is not valid UTF-8: {}",
                    path.display()
                )));
            }
            (
                std::str::from_utf8(&buf[..valid])
                    .expect("valid prefix")
                    .to_string(),
                true,
            )
        }
    };

    let truncated = file_size > max_bytes || invalid;
    Ok((text, truncated, bytes_read))
}

fn mk_empty_profile(owner_id: Uuid) -> crate::agents::UserProfileEntry {
    crate::agents::UserProfileEntry {
        id: owner_id,
        profile: serde_json::json!({}),
        updated_at: Utc::now(),
    }
}

fn validate_block_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(crate::Error::InvalidInput(
            "Profile block name cannot be empty".into(),
        ));
    }
    if trimmed.len() > 64 {
        return Err(crate::Error::InvalidInput(
            "Profile block name must be <= 64 characters".into(),
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(crate::Error::InvalidInput(
            "Profile block name must be [a-zA-Z0-9._-]".into(),
        ));
    }
    Ok(())
}

fn prompt_sort_key(block: &ProfileBlock) -> (u8, u8, String) {
    let scope_weight = match block.scope {
        ProfileBlockScope::Global => 0,
        ProfileBlockScope::Project => 1,
        ProfileBlockScope::Agent => 2,
    };

    let lower = block.name.to_lowercase();
    let reserved_weight = if lower == RESERVED_BLOCK_PERSONA {
        0
    } else if lower == RESERVED_BLOCK_HUMAN {
        1
    } else {
        2
    };

    (scope_weight, reserved_weight, lower)
}

fn extract_blocks(profile: &Value) -> Vec<ProfileBlock> {
    let Some(obj) = profile.as_object() else {
        return Vec::new();
    };
    let Some(blocks) = obj.get("blocks").and_then(|v| v.as_object()) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (name, value) in blocks {
        let Some(block_obj) = value.as_object() else {
            continue;
        };

        let content = block_obj
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let scope = block_obj
            .get("scope")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<ProfileBlockScope>().ok())
            .unwrap_or(ProfileBlockScope::Project);

        let updated_at = block_obj
            .get("updated_at")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        out.push(ProfileBlock {
            name: name.clone(),
            scope,
            content,
            updated_at,
        });
    }

    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.scope.cmp(&b.scope)));
    out
}

fn upsert_block_in_profile(
    profile: &mut Value,
    name: &str,
    scope: ProfileBlockScope,
    content: &str,
    updated_at: DateTime<Utc>,
) -> Result<()> {
    if !profile.is_object() {
        *profile = serde_json::json!({});
    }

    let obj = profile.as_object_mut().expect("object");
    let blocks = obj
        .entry("blocks")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

    if !blocks.is_object() {
        *blocks = serde_json::Value::Object(serde_json::Map::new());
    }

    blocks.as_object_mut().expect("object").insert(
        name.to_string(),
        serde_json::json!({
            "content": content,
            "scope": scope.as_str(),
            "updated_at": updated_at.to_rfc3339(),
        }),
    );

    Ok(())
}

fn apply_patch(mut content: String, ops: &[ProfileBlockPatchOp]) -> Result<String> {
    let mut lines: Vec<String> = content.lines().map(ToString::to_string).collect();
    for op in ops {
        match op {
            ProfileBlockPatchOp::Insert { before_line, text } => {
                let idx = before_line.saturating_sub(1);
                if idx > lines.len() {
                    return Err(crate::Error::InvalidInput(format!(
                        "Insert before_line out of range ({} > {})",
                        before_line,
                        lines.len() + 1
                    )));
                }
                let insert_lines: Vec<String> = text.lines().map(ToString::to_string).collect();
                lines.splice(idx..idx, insert_lines);
            }
            ProfileBlockPatchOp::Replace {
                start_line,
                end_line,
                text,
            } => {
                let start_line = *start_line;
                let end_line = *end_line;
                if end_line < start_line {
                    return Err(crate::Error::InvalidInput(
                        "Replace end_line must be >= start_line".into(),
                    ));
                }
                let start = start_line.saturating_sub(1);
                let end_exclusive = end_line;
                if start >= lines.len() || end_exclusive > lines.len() {
                    return Err(crate::Error::InvalidInput(format!(
                        "Replace range out of bounds ({start_line}..={end_line}, lines={})",
                        lines.len()
                    )));
                }
                let replace_lines: Vec<String> = text.lines().map(ToString::to_string).collect();
                lines.splice(start..end_exclusive, replace_lines);
            }
        }
    }

    content.clear();
    for (idx, line) in lines.iter().enumerate() {
        content.push_str(line);
        if idx < lines.len().saturating_sub(1) {
            content.push('\n');
        }
    }
    Ok(content)
}

async fn record_profile_block_event(
    pool: &SqlitePool,
    actor_id: Uuid,
    span_id: Option<String>,
    owner_id: Uuid,
    payload: Value,
) -> Result<()> {
    ensure_agent_exists(pool, actor_id, owner_id).await?;
    let mut event = AgentEvent::new(actor_id, "profile_block_update");
    event.span_id = span_id;
    event.payload = payload;
    operations::record_agent_event(pool, &event).await?;
    Ok(())
}

async fn ensure_agent_exists(pool: &SqlitePool, actor_id: Uuid, owner_id: Uuid) -> Result<()> {
    if operations::get_agent(pool, actor_id).await?.is_some() {
        return Ok(());
    }

    let (name, kind) = if actor_id == owner_id {
        ("user", "human")
    } else {
        ("agent", "agent")
    };

    let mut record = AgentRecord::new(name, kind);
    record.id = actor_id;
    operations::upsert_agent(pool, &record).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentRecord;
    use crate::database::Database;
    use std::fs;

    #[tokio::test]
    async fn set_and_patch_profile_block_records_agent_events() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut config = Config::default();
        config.stores.directory = temp.path().join("stores");
        config.stores.default = "test".to_string();
        let db = Database::init_store(&config, None).await?;

        let owner = Uuid::new_v4();
        let mut actor = AgentRecord::new("actor", "human");
        actor.id = owner;
        operations::upsert_agent(db.pool(), &actor).await?;

        let svc = ProfileBlocksService::from_config(&config);
        let initial_events = operations::count_agent_events(db.pool()).await?;

        let block = svc
            .set_block(
                db.pool(),
                owner,
                RESERVED_BLOCK_PERSONA,
                "line1\nline2".to_string(),
                ProfileBlockWriteContext {
                    scope: ProfileBlockScope::Global,
                    actor_id: owner,
                    span_id: None,
                },
            )
            .await?;
        assert_eq!(block.name, RESERVED_BLOCK_PERSONA);
        assert_eq!(block.scope, ProfileBlockScope::Global);

        let patched = svc
            .patch_block(
                db.pool(),
                owner,
                RESERVED_BLOCK_PERSONA,
                vec![ProfileBlockPatchOp::Replace {
                    start_line: 2,
                    end_line: 2,
                    text: "updated".to_string(),
                }],
                ProfileBlockWriteContext {
                    scope: ProfileBlockScope::Global,
                    actor_id: owner,
                    span_id: Some("span-1".to_string()),
                },
            )
            .await?;
        assert_eq!(patched.content, "line1\nupdated");
        assert_eq!(patched.scope, ProfileBlockScope::Global);

        let listed = svc.list_blocks(db.pool(), owner).await?;
        assert_eq!(listed.len(), 1);

        let after_events = operations::count_agent_events(db.pool()).await?;
        assert_eq!(after_events - initial_events, 2);

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn ingest_directory_creates_scoped_blocks() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut config = Config::default();
        config.stores.directory = temp.path().join("stores");
        config.stores.default = "test".to_string();
        config.profile_blocks.max_block_chars = 512;
        let db = Database::init_store(&config, None).await?;

        let root = temp.path().join("repo");
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("README.md"), "# Hello\nworld\n")?;
        fs::write(root.join(".hidden.md"), "nope")?;
        fs::write(root.join("src").join("lib.rs"), "pub fn hi() {}\n")?;

        let owner = Uuid::new_v4();
        let mut actor = AgentRecord::new("actor", "human");
        actor.id = owner;
        operations::upsert_agent(db.pool(), &actor).await?;

        let svc = ProfileBlocksService::from_config(&config);
        let result = svc
            .ingest_directory(
                db.pool(),
                owner,
                &root,
                ProfileBlocksIngestOptions {
                    scope: ProfileBlockScope::Project,
                    prefix: "repo".to_string(),
                    extensions: vec!["md".to_string(), "rs".to_string()],
                    ..ProfileBlocksIngestOptions::default()
                },
                owner,
                None,
            )
            .await?;

        assert!(result.files_seen >= 2);
        assert_eq!(result.scope, ProfileBlockScope::Project);
        assert!(!result.blocks.is_empty());
        assert!(result
            .blocks
            .iter()
            .all(|b| b.scope == ProfileBlockScope::Project));
        assert!(result
            .blocks
            .iter()
            .flat_map(|b| b.files.iter())
            .all(|p| !p.starts_with('.')));

        let blocks = svc.list_blocks(db.pool(), owner).await?;
        assert!(blocks.iter().any(|b| b.scope == ProfileBlockScope::Project));
        assert!(blocks
            .iter()
            .all(|b| b.content.chars().count() <= config.profile_blocks.max_block_chars));

        db.close().await;
        Ok(())
    }
}
