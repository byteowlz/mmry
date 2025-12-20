use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::agents::AgentEvent;
use crate::agents::AgentRecord;
use crate::config::Config;
use crate::database::operations;
use crate::Result;

pub const RESERVED_BLOCK_PERSONA: &str = "persona";
pub const RESERVED_BLOCK_HUMAN: &str = "human";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileBlock {
    pub name: String,
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
}

impl ProfileBlocksService {
    pub fn from_config(config: &Config) -> Self {
        Self {
            max_block_chars: config.profile_blocks.max_block_chars,
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
    ) -> Result<Option<ProfileBlock>> {
        let profile = operations::get_user_profile(pool, owner_id).await?;
        let Some(profile) = profile else {
            return Ok(None);
        };

        Ok(extract_blocks(&profile.profile)
            .into_iter()
            .find(|b| b.name == name))
    }

    pub async fn set_block(
        &self,
        pool: &SqlitePool,
        owner_id: Uuid,
        name: &str,
        content: String,
        actor_id: Uuid,
        span_id: Option<String>,
    ) -> Result<ProfileBlock> {
        validate_block_name(name)?;
        self.validate_block_content(&content)?;

        let mut profile = operations::get_user_profile(pool, owner_id)
            .await?
            .unwrap_or_else(|| mk_empty_profile(owner_id));

        let now = Utc::now();
        upsert_block_in_profile(&mut profile.profile, name, &content, now)?;
        profile.updated_at = now;
        operations::set_user_profile(pool, &profile).await?;

        record_profile_block_event(
            pool,
            actor_id,
            span_id,
            owner_id,
            serde_json::json!({
                "operation": "set",
                "owner_id": owner_id,
                "block": name,
                "chars": content.chars().count(),
            }),
        )
        .await?;

        Ok(ProfileBlock {
            name: name.to_string(),
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
        actor_id: Uuid,
        span_id: Option<String>,
    ) -> Result<ProfileBlock> {
        validate_block_name(name)?;

        let mut profile = operations::get_user_profile(pool, owner_id)
            .await?
            .unwrap_or_else(|| mk_empty_profile(owner_id));

        let existing = extract_blocks(&profile.profile)
            .into_iter()
            .find(|b| b.name == name)
            .map(|b| b.content)
            .unwrap_or_default();

        let updated = apply_patch(existing, &ops)?;
        self.validate_block_content(&updated)?;

        let now = Utc::now();
        upsert_block_in_profile(&mut profile.profile, name, &updated, now)?;
        profile.updated_at = now;
        operations::set_user_profile(pool, &profile).await?;

        record_profile_block_event(
            pool,
            actor_id,
            span_id,
            owner_id,
            serde_json::json!({
                "operation": "patch",
                "owner_id": owner_id,
                "block": name,
                "chars": updated.chars().count(),
                "ops": ops,
            }),
        )
        .await?;

        Ok(ProfileBlock {
            name: name.to_string(),
            content: updated,
            updated_at: now,
        })
    }

    pub fn render_for_prompt(&self, blocks: &[ProfileBlock]) -> String {
        let mut sorted = blocks.to_vec();
        sorted.sort_by(|a, b| prompt_sort_key(&a.name).cmp(&prompt_sort_key(&b.name)));

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

fn prompt_sort_key(name: &str) -> (u8, String) {
    let lower = name.to_lowercase();
    if lower == RESERVED_BLOCK_PERSONA {
        return (0, lower);
    }
    if lower == RESERVED_BLOCK_HUMAN {
        return (1, lower);
    }
    (2, lower)
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

        let updated_at = block_obj
            .get("updated_at")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        out.push(ProfileBlock {
            name: name.clone(),
            content,
            updated_at,
        });
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn upsert_block_in_profile(
    profile: &mut Value,
    name: &str,
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
                owner,
                None,
            )
            .await?;
        assert_eq!(block.name, RESERVED_BLOCK_PERSONA);

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
                owner,
                Some("span-1".to_string()),
            )
            .await?;
        assert_eq!(patched.content, "line1\nupdated");

        let listed = svc.list_blocks(db.pool(), owner).await?;
        assert_eq!(listed.len(), 1);

        let after_events = operations::count_agent_events(db.pool()).await?;
        assert_eq!(after_events - initial_events, 2);

        db.close().await;
        Ok(())
    }
}
