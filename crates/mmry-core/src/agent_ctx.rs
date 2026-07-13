//! Deterministic source metadata captured from the `AGENT_CTX_*` contract.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

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
    std::env::var(name).ok().and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

impl AgentCtx {
    pub fn from_env() -> Self {
        Self {
            version: read("AGENT_CTX_VERSION"),
            platform_name: read("AGENT_CTX_PLATFORM_NAME"),
            platform_version: read("AGENT_CTX_PLATFORM_VERSION"),
            harness: read("AGENT_CTX_HARNESS"),
            run_mode: read("AGENT_CTX_RUN_MODE"),
            platform_session_id: read("AGENT_CTX_PLATFORM_SESSION_ID"),
            harness_session_id: read("AGENT_CTX_HARNESS_SESSION_ID"),
            session_name: read("AGENT_CTX_SESSION_NAME"),
            readable_id: read("AGENT_CTX_READABLE_ID"),
            workspace_id: read("AGENT_CTX_WORKSPACE_ID"),
            workspace_path: read("AGENT_CTX_WORKSPACE_PATH"),
            user_id: read("AGENT_CTX_USER_ID"),
            model: read("AGENT_CTX_MODEL"),
            request_id: read("AGENT_CTX_REQUEST_ID"),
            correlation_id: read("AGENT_CTX_CORRELATION_ID"),
            sandbox_profile: read("AGENT_CTX_SANDBOX_PROFILE"),
        }
    }

    pub fn as_json(&self) -> Value {
        let value = serde_json::to_value(self).unwrap_or(Value::Null);
        let Value::Object(object) = value else {
            return Value::Object(Map::new());
        };
        Value::Object(
            object
                .into_iter()
                .filter(|(_, value)| !value.is_null())
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_context_is_an_empty_object() {
        assert_eq!(AgentCtx::default().as_json(), serde_json::json!({}));
    }

    #[test]
    fn populated_context_omits_missing_fields() {
        let context = AgentCtx {
            harness: Some("pi".into()),
            ..AgentCtx::default()
        };
        assert_eq!(context.as_json(), serde_json::json!({"harness": "pi"}));
    }
}
