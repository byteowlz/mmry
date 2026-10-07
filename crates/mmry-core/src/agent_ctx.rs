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
    /// Producer-supplied execution environment/profile label (open vocabulary;
    /// absent means unknown).
    pub exec_env: Option<String>,
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

/// Validate `AGENT_CTX_EXEC_ENV` against the v3 bounded-string contract:
/// max length 256 and no control characters. Out-of-bounds values are treated
/// as unknown (None), never a fabricated or malformed profile.
fn bounded_exec_env_ok(value: &str) -> bool {
    value.chars().count() <= 256 && !value.chars().any(|c| c.is_control())
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
        let read_exec_env = |name: &str| {
            lookup(&format!("AGENT_CTX_{name}"))
                .and_then(|v| clean(Some(v)))
                .filter(|v| bounded_exec_env_ok(v))
        };
        Self {
            version: read("VERSION"),
            platform_name: read("PLATFORM_NAME"),
            platform_version: read("PLATFORM_VERSION"),
            harness: read("HARNESS"),
            run_mode: read("RUN_MODE"),
            exec_env: read_exec_env("EXEC_ENV"),
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

    #[test]
    fn reads_v3_exec_env_from_env() {
        let context = AgentCtx::from_lookup(|key| match key {
            "AGENT_CTX_VERSION" => Some("3".into()),
            "AGENT_CTX_EXEC_ENV" => Some("linux-container".into()),
            _ => None,
        });
        let json = context.as_json();
        assert_eq!(json["version"], "3");
        assert_eq!(json["exec_env"], "linux-container");
        // EXEC_ENV absent from the env means unknown, not a fabricated value.
        assert!(AgentCtx::from_lookup(|_| None).as_json().get("exec_env").is_none());
    }

    #[test]
    fn exec_env_bounded_validation() {
        let from = |value: &str| AgentCtx::from_lookup(|k| {
            (k == "AGENT_CTX_EXEC_ENV").then(|| value.to_string())
        });
        let cases: &[(&str, bool)] = &[
            ("linux-container", true),
            ("new-runtime-profile", true),
            ("", false),
            ("   ", false),
            ("bad\nvalue", false),
        ];
        for (value, expected_present) in cases {
            let json = from(value).as_json();
            assert_eq!(
                json.get("exec_env").is_some(),
                *expected_present,
                "value={value:?}"
            );
        }
        // Oversize (257 chars) is treated as unknown; 256 is the boundary.
        assert!(from(&"x".repeat(257)).as_json().get("exec_env").is_none());
        assert_eq!(from(&"y".repeat(256)).as_json()["exec_env"], "y".repeat(256));
    }

    #[test]
    fn stored_record_new_and_old_round_trips() {
        // New record with exec_env serializes and deserializes.
        let new = AgentCtx {
            version: Some("3".into()),
            exec_env: Some("windows-vm".into()),
            harness: Some("pi".into()),
            ..AgentCtx::default()
        };
        let json = serde_json::to_string(&new).unwrap();
        let parsed: AgentCtx = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.exec_env.as_deref(), Some("windows-vm"));
        assert_eq!(parsed.harness.as_deref(), Some("pi"));

        // Legacy stored record with run_mode and no exec_env field still loads;
        // exec_env is None, run_mode is preserved, neither relabeled.
        let mut legacy = serde_json::json!({
            "version": "2",
            "run_mode": "local",
            "harness": "pi",
        });
        legacy.as_object_mut().unwrap().remove("exec_env");
        let old: AgentCtx = serde_json::from_value(legacy).unwrap();
        assert!(old.exec_env.is_none());
        assert_eq!(old.run_mode.as_deref(), Some("local"));
        assert_eq!(old.harness.as_deref(), Some("pi"));
    }
}
