//! AGENT_CTX environment contract (v1) — read-side support.
//!
//! See `../../../../schemas/agent-context-env/agent-context-env.md`.
//!
//! Mmry consumes `AGENT_CTX_*` env vars defensively: missing or malformed
//! values never break tool behavior. Captured context is stamped into
//! `memory.metadata.agent_ctx` and denormalized into indexed columns
//! (`workspace_id`, `platform_session_id`, `harness_session_id`) at insert time.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

const ENV_VERSION: &str = "AGENT_CTX_VERSION";
const ENV_PLATFORM_NAME: &str = "AGENT_CTX_PLATFORM_NAME";
const ENV_PLATFORM_VERSION: &str = "AGENT_CTX_PLATFORM_VERSION";
const ENV_HARNESS: &str = "AGENT_CTX_HARNESS";
const ENV_RUN_MODE: &str = "AGENT_CTX_RUN_MODE";
const ENV_PLATFORM_SESSION_ID: &str = "AGENT_CTX_PLATFORM_SESSION_ID";
const ENV_HARNESS_SESSION_ID: &str = "AGENT_CTX_HARNESS_SESSION_ID";
const ENV_SESSION_NAME: &str = "AGENT_CTX_SESSION_NAME";
const ENV_READABLE_ID: &str = "AGENT_CTX_READABLE_ID";
const ENV_WORKSPACE_ID: &str = "AGENT_CTX_WORKSPACE_ID";
const ENV_WORKSPACE_PATH: &str = "AGENT_CTX_WORKSPACE_PATH";
const ENV_USER_ID: &str = "AGENT_CTX_USER_ID";
const ENV_MODEL: &str = "AGENT_CTX_MODEL";
const ENV_REQUEST_ID: &str = "AGENT_CTX_REQUEST_ID";
const ENV_CORRELATION_ID: &str = "AGENT_CTX_CORRELATION_ID";
const ENV_SANDBOX_PROFILE: &str = "AGENT_CTX_SANDBOX_PROFILE";

/// Snapshot of `AGENT_CTX_*` runtime metadata.
///
/// All fields are optional; consumers must tolerate any missing field.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCtx {
    pub version: Option<String>,
    pub platform_name: Option<String>,
    pub platform_version: Option<String>,
    pub harness: Option<String>,
    pub run_mode: Option<String>,
    pub platform_session_id: Option<String>,
    pub harness_session_id: Option<String>,
    pub session_name: Option<String>,
    pub readable_id: Option<String>,
    pub workspace_id: Option<String>,
    pub workspace_path: Option<String>,
    pub user_id: Option<String>,
    pub model: Option<String>,
    pub request_id: Option<String>,
    pub correlation_id: Option<String>,
    pub sandbox_profile: Option<String>,
}

fn read(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(_) => None,
    }
}

impl AgentCtx {
    /// Read every `AGENT_CTX_*` variable from the process environment.
    /// Defensive: empty/whitespace values are treated as missing.
    pub fn from_env() -> Self {
        Self {
            version: read(ENV_VERSION),
            platform_name: read(ENV_PLATFORM_NAME),
            platform_version: read(ENV_PLATFORM_VERSION),
            harness: read(ENV_HARNESS),
            run_mode: read(ENV_RUN_MODE),
            platform_session_id: read(ENV_PLATFORM_SESSION_ID),
            harness_session_id: read(ENV_HARNESS_SESSION_ID),
            session_name: read(ENV_SESSION_NAME),
            readable_id: read(ENV_READABLE_ID),
            workspace_id: read(ENV_WORKSPACE_ID),
            workspace_path: read(ENV_WORKSPACE_PATH),
            user_id: read(ENV_USER_ID),
            model: read(ENV_MODEL),
            request_id: read(ENV_REQUEST_ID),
            correlation_id: read(ENV_CORRELATION_ID),
            sandbox_profile: read(ENV_SANDBOX_PROFILE),
        }
    }

    /// True when no fields were captured.
    pub fn is_empty(&self) -> bool {
        self.version.is_none()
            && self.platform_name.is_none()
            && self.platform_version.is_none()
            && self.harness.is_none()
            && self.run_mode.is_none()
            && self.platform_session_id.is_none()
            && self.harness_session_id.is_none()
            && self.session_name.is_none()
            && self.readable_id.is_none()
            && self.workspace_id.is_none()
            && self.workspace_path.is_none()
            && self.user_id.is_none()
            && self.model.is_none()
            && self.request_id.is_none()
            && self.correlation_id.is_none()
            && self.sandbox_profile.is_none()
    }

    /// Render the populated subset as a JSON object with snake_case keys
    /// (the `agent_ctx_` prefix is dropped). Missing fields are omitted.
    pub fn as_json(&self) -> Value {
        let mut obj = Map::new();
        let mut put = |k: &str, v: &Option<String>| {
            if let Some(v) = v {
                obj.insert(k.to_string(), Value::String(v.clone()));
            }
        };
        put("version", &self.version);
        put("platform_name", &self.platform_name);
        put("platform_version", &self.platform_version);
        put("harness", &self.harness);
        put("run_mode", &self.run_mode);
        put("platform_session_id", &self.platform_session_id);
        put("harness_session_id", &self.harness_session_id);
        put("session_name", &self.session_name);
        put("readable_id", &self.readable_id);
        put("workspace_id", &self.workspace_id);
        put("workspace_path", &self.workspace_path);
        put("user_id", &self.user_id);
        put("model", &self.model);
        put("request_id", &self.request_id);
        put("correlation_id", &self.correlation_id);
        put("sandbox_profile", &self.sandbox_profile);
        Value::Object(obj)
    }

    /// Stamp the snapshot under the `agent_ctx` key inside a metadata object.
    /// If `metadata` is not a JSON object it is replaced with a fresh one.
    /// No-op when the snapshot is empty.
    pub fn merge_into_metadata(&self, metadata: &mut Value) {
        if self.is_empty() {
            return;
        }
        if !metadata.is_object() {
            *metadata = Value::Object(Map::new());
        }
        let obj = metadata.as_object_mut().expect("metadata is object");
        obj.insert("agent_ctx".to_string(), self.as_json());
    }

    /// Promote ctx fields into a flat `AgentRecord.metadata` JSON object so
    /// existing `repo()` / `workspace()` / `session_id()` accessors keep
    /// working without forcing callers to know about `AGENT_CTX_*`.
    ///
    /// Existing keys are preserved; ctx only fills in missing slots.
    pub fn enrich_agent_meta(&self, meta: &mut Value) {
        if self.is_empty() {
            return;
        }
        if !meta.is_object() {
            *meta = Value::Object(Map::new());
        }
        let obj = meta.as_object_mut().expect("meta is object");

        let mut fill = |key: &str, value: &Option<String>| {
            if let Some(v) = value {
                obj.entry(key.to_string())
                    .or_insert_with(|| Value::String(v.clone()));
            }
        };
        fill("workspace", &self.workspace_path);
        fill("workspace_id", &self.workspace_id);
        fill("session_id", &self.platform_session_id);
        fill("harness_session_id", &self.harness_session_id);
        fill("user_id", &self.user_id);
        fill("harness", &self.harness);
        fill("platform", &self.platform_name);
        fill("model", &self.model);

        // Always carry the full structured snapshot for forward-compat.
        obj.entry("agent_ctx".to_string())
            .or_insert_with(|| self.as_json());
    }

    /// Suggested fallback agent name when no explicit identity was given.
    /// Returns the harness id (e.g. "pi"), platform name, or None.
    pub fn default_agent_name(&self) -> Option<String> {
        self.harness.clone().or_else(|| self.platform_name.clone())
    }

    /// Suggested fallback agent kind ("coding_agent" if any harness/platform
    /// is detected, otherwise None — caller decides the ultimate default).
    pub fn default_agent_kind(&self) -> Option<String> {
        if self.harness.is_some() || self.platform_name.is_some() {
            Some("coding_agent".to_string())
        } else {
            None
        }
    }

    /// Borrow the indexed-column triple in one shot for SQL writes.
    pub fn index_keys(&self) -> CtxIndexKeys<'_> {
        CtxIndexKeys {
            workspace_id: self.workspace_id.as_deref(),
            platform_session_id: self.platform_session_id.as_deref(),
            harness_session_id: self.harness_session_id.as_deref(),
        }
    }
}

/// The three stable IDs that mmry denormalizes into indexed columns.
#[derive(Debug, Clone, Copy, Default)]
pub struct CtxIndexKeys<'a> {
    pub workspace_id: Option<&'a str>,
    pub platform_session_id: Option<&'a str>,
    pub harness_session_id: Option<&'a str>,
}

impl<'a> CtxIndexKeys<'a> {
    /// Read the same keys out of a memory's `metadata.agent_ctx` JSON blob.
    /// Used at insert time so callers can stamp via `merge_into_metadata`
    /// alone and still get column-backed indexes.
    pub fn from_metadata(metadata: &'a Value) -> Self {
        let ctx = metadata.get("agent_ctx").and_then(Value::as_object);
        let pick =
            |key: &str| -> Option<&'a str> { ctx.and_then(|c| c.get(key)).and_then(Value::as_str) };
        Self {
            workspace_id: pick("workspace_id"),
            platform_session_id: pick("platform_session_id"),
            harness_session_id: pick("harness_session_id"),
        }
    }
}

/// Read the current `AGENT_CTX_*` env and stamp it onto a memory's metadata
/// in one call. No-op when no ctx vars are set. Convenience for hot paths
/// (ingest, watchers) that don't want to keep an `AgentCtx` around.
pub fn stamp_env_ctx(metadata: &mut Value) {
    AgentCtx::from_env().merge_into_metadata(metadata);
}

/// Stamp `agent_ctx` JSON onto an arbitrary metadata `Value`, used by tests
/// and helpers that don't want to construct an `AgentCtx` themselves.
pub fn merge_ctx_value_into_metadata(ctx: &Value, metadata: &mut Value) {
    if !ctx.is_object() || ctx.as_object().map(Map::is_empty).unwrap_or(true) {
        return;
    }
    if !metadata.is_object() {
        *metadata = Value::Object(Map::new());
    }
    metadata
        .as_object_mut()
        .expect("metadata is object")
        .insert("agent_ctx".to_string(), ctx.clone());
}

/// Convenience constructor for tests: build a ctx from a list of (env, value)
/// pairs without touching the real process environment.
#[cfg(test)]
pub(crate) fn from_pairs(pairs: &[(&str, &str)]) -> AgentCtx {
    let lookup = |name: &str| -> Option<String> {
        pairs
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.to_string())
            .filter(|v| !v.trim().is_empty())
    };
    AgentCtx {
        version: lookup(ENV_VERSION),
        platform_name: lookup(ENV_PLATFORM_NAME),
        platform_version: lookup(ENV_PLATFORM_VERSION),
        harness: lookup(ENV_HARNESS),
        run_mode: lookup(ENV_RUN_MODE),
        platform_session_id: lookup(ENV_PLATFORM_SESSION_ID),
        harness_session_id: lookup(ENV_HARNESS_SESSION_ID),
        session_name: lookup(ENV_SESSION_NAME),
        readable_id: lookup(ENV_READABLE_ID),
        workspace_id: lookup(ENV_WORKSPACE_ID),
        workspace_path: lookup(ENV_WORKSPACE_PATH),
        user_id: lookup(ENV_USER_ID),
        model: lookup(ENV_MODEL),
        request_id: lookup(ENV_REQUEST_ID),
        correlation_id: lookup(ENV_CORRELATION_ID),
        sandbox_profile: lookup(ENV_SANDBOX_PROFILE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_ctx_renders_empty_json_object_and_does_not_stamp_metadata() {
        let ctx = AgentCtx::default();
        assert!(ctx.is_empty());
        assert_eq!(ctx.as_json(), json!({}));

        let mut meta = json!({"existing": "stays"});
        ctx.merge_into_metadata(&mut meta);
        assert_eq!(meta, json!({"existing": "stays"}));
    }

    #[test]
    fn populated_ctx_serializes_only_present_fields() {
        let ctx = from_pairs(&[
            ("AGENT_CTX_VERSION", "1"),
            ("AGENT_CTX_HARNESS", "pi"),
            ("AGENT_CTX_WORKSPACE_ID", "ws_abc"),
            ("AGENT_CTX_WORKSPACE_PATH", ""), // empty -> dropped
        ]);
        assert!(!ctx.is_empty());
        let value = ctx.as_json();
        assert_eq!(
            value,
            json!({
                "version": "1",
                "harness": "pi",
                "workspace_id": "ws_abc",
            })
        );
    }

    #[test]
    fn merge_into_metadata_stamps_under_agent_ctx_key() {
        let ctx = from_pairs(&[
            ("AGENT_CTX_HARNESS", "pi"),
            ("AGENT_CTX_PLATFORM_SESSION_ID", "sess_8f"),
        ]);
        let mut meta = json!({"keep": true});
        ctx.merge_into_metadata(&mut meta);

        assert_eq!(meta["keep"], json!(true));
        assert_eq!(meta["agent_ctx"]["harness"], json!("pi"));
        assert_eq!(meta["agent_ctx"]["platform_session_id"], json!("sess_8f"));
    }

    #[test]
    fn merge_into_metadata_replaces_non_object_values() {
        let ctx = from_pairs(&[("AGENT_CTX_HARNESS", "pi")]);
        let mut meta = Value::Null;
        ctx.merge_into_metadata(&mut meta);
        assert!(meta.is_object());
        assert_eq!(meta["agent_ctx"]["harness"], json!("pi"));
    }

    #[test]
    fn enrich_agent_meta_fills_missing_slots_only() {
        let ctx = from_pairs(&[
            ("AGENT_CTX_HARNESS", "pi"),
            ("AGENT_CTX_PLATFORM_SESSION_ID", "sess_8f"),
            ("AGENT_CTX_WORKSPACE_PATH", "/repo"),
        ]);
        let mut meta = json!({"workspace": "/explicit"});
        ctx.enrich_agent_meta(&mut meta);

        // Caller's explicit workspace is preserved.
        assert_eq!(meta["workspace"], json!("/explicit"));
        assert_eq!(meta["session_id"], json!("sess_8f"));
        assert_eq!(meta["harness"], json!("pi"));
        assert_eq!(meta["agent_ctx"]["harness"], json!("pi"));
    }

    #[test]
    fn default_agent_name_prefers_harness_then_platform() {
        let pi = from_pairs(&[
            ("AGENT_CTX_HARNESS", "pi"),
            ("AGENT_CTX_PLATFORM_NAME", "oqto"),
        ]);
        assert_eq!(pi.default_agent_name(), Some("pi".to_string()));

        let oqto_only = from_pairs(&[("AGENT_CTX_PLATFORM_NAME", "oqto")]);
        assert_eq!(oqto_only.default_agent_name(), Some("oqto".to_string()));

        let none = AgentCtx::default();
        assert_eq!(none.default_agent_name(), None);
    }

    #[test]
    fn index_keys_round_trip_via_metadata() {
        let ctx = from_pairs(&[
            ("AGENT_CTX_WORKSPACE_ID", "ws_a13f"),
            ("AGENT_CTX_PLATFORM_SESSION_ID", "sess_8f"),
            ("AGENT_CTX_HARNESS_SESSION_ID", "pi_d91c"),
        ]);
        let mut meta = json!({});
        ctx.merge_into_metadata(&mut meta);

        let keys = CtxIndexKeys::from_metadata(&meta);
        assert_eq!(keys.workspace_id, Some("ws_a13f"));
        assert_eq!(keys.platform_session_id, Some("sess_8f"));
        assert_eq!(keys.harness_session_id, Some("pi_d91c"));
    }
}
