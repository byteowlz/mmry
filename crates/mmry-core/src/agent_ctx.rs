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
    pub agent_id: Option<String>,
    pub agent_address: Option<String>,
    /// Stable machine key (not the hostname); see [`current_machine`].
    pub machine_id: Option<String>,
    pub node_hostname: Option<String>,
    pub multiplexer: Option<String>,
    pub os_arch: Option<String>,
}

fn clean(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

impl AgentCtx {
    /// Read `AGENT_CTX_*` from the process environment. Any
    /// `AGENT_CTX_VERSION` is accepted; unknown variables are ignored.
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Like [`AgentCtx::from_env`] with an explicit variable lookup.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let read = |name: &str| clean(lookup(&format!("AGENT_CTX_{name}")));
        Self {
            version: read("VERSION"),
            platform_name: read("PLATFORM_NAME"),
            platform_version: read("PLATFORM_VERSION"),
            harness: read("HARNESS"),
            run_mode: read("RUN_MODE"),
            platform_session_id: read("PLATFORM_SESSION_ID"),
            harness_session_id: read("HARNESS_SESSION_ID"),
            session_name: read("SESSION_NAME"),
            readable_id: read("READABLE_ID"),
            workspace_id: read("WORKSPACE_ID"),
            workspace_path: read("WORKSPACE_PATH"),
            user_id: read("USER_ID"),
            model: read("MODEL"),
            request_id: read("REQUEST_ID"),
            correlation_id: read("CORRELATION_ID"),
            sandbox_profile: read("SANDBOX_PROFILE"),
            agent_id: read("AGENT_ID"),
            agent_address: read("AGENT_ADDRESS"),
            machine_id: read("MACHINE_ID"),
            node_hostname: read("NODE_HOSTNAME"),
            multiplexer: read("MULTIPLEXER"),
            os_arch: read("OS_ARCH"),
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

/// Label for "this machine": `AGENT_CTX_MACHINE_ID`, else the host name.
pub fn current_machine(context: &AgentCtx) -> Option<String> {
    context
        .machine_id
        .clone()
        .or_else(|| clean(std::fs::read_to_string("/proc/sys/kernel/hostname").ok()))
        .or_else(|| clean(std::env::var("COMPUTERNAME").ok()))
        .or_else(|| clean(std::env::var("HOSTNAME").ok()))
        .or_else(|| {
            let output = std::process::Command::new("hostname").output().ok()?;
            clean(String::from_utf8(output.stdout).ok())
        })
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

    #[test]
    fn reads_v2_fields_and_ignores_unknown_ones() {
        let context = AgentCtx::from_lookup(|key| match key {
            "AGENT_CTX_VERSION" => Some("7".into()),
            "AGENT_CTX_AGENT_ID" => Some("agent-1".into()),
            "AGENT_CTX_MACHINE_ID" => Some(" m-42 ".into()),
            "AGENT_CTX_NODE_HOSTNAME" => Some(String::new()),
            "AGENT_CTX_FUTURE_THING" => Some("x".into()),
            _ => None,
        });
        assert_eq!(
            context.as_json(),
            serde_json::json!({"version": "7", "agent_id": "agent-1", "machine_id": "m-42"})
        );
        assert_eq!(current_machine(&context).as_deref(), Some("m-42"));
        assert!(current_machine(&AgentCtx::default()).is_some());
    }
}
