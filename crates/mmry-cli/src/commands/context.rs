use clap::Parser;
use mmry_core::agent_ctx::AgentCtx;
use mmry_core::config::Config;
use serde_json::json;

#[derive(Parser)]
pub struct ContextCmd {
    #[arg(long, help = "Output as JSON")]
    pub json: bool,
}

const FIELDS: &[(&str, &str)] = &[
    ("AGENT_CTX_VERSION", "version"),
    ("AGENT_CTX_PLATFORM_NAME", "platform_name"),
    ("AGENT_CTX_PLATFORM_VERSION", "platform_version"),
    ("AGENT_CTX_HARNESS", "harness"),
    ("AGENT_CTX_RUN_MODE", "run_mode"),
    ("AGENT_CTX_PLATFORM_SESSION_ID", "platform_session_id"),
    ("AGENT_CTX_HARNESS_SESSION_ID", "harness_session_id"),
    ("AGENT_CTX_SESSION_NAME", "session_name"),
    ("AGENT_CTX_READABLE_ID", "readable_id"),
    ("AGENT_CTX_WORKSPACE_ID", "workspace_id"),
    ("AGENT_CTX_WORKSPACE_PATH", "workspace_path"),
    ("AGENT_CTX_USER_ID", "user_id"),
    ("AGENT_CTX_MODEL", "model"),
    ("AGENT_CTX_REQUEST_ID", "request_id"),
    ("AGENT_CTX_CORRELATION_ID", "correlation_id"),
    ("AGENT_CTX_SANDBOX_PROFILE", "sandbox_profile"),
];

fn field_value<'a>(ctx: &'a AgentCtx, key: &str) -> Option<&'a str> {
    match key {
        "version" => ctx.version.as_deref(),
        "platform_name" => ctx.platform_name.as_deref(),
        "platform_version" => ctx.platform_version.as_deref(),
        "harness" => ctx.harness.as_deref(),
        "run_mode" => ctx.run_mode.as_deref(),
        "platform_session_id" => ctx.platform_session_id.as_deref(),
        "harness_session_id" => ctx.harness_session_id.as_deref(),
        "session_name" => ctx.session_name.as_deref(),
        "readable_id" => ctx.readable_id.as_deref(),
        "workspace_id" => ctx.workspace_id.as_deref(),
        "workspace_path" => ctx.workspace_path.as_deref(),
        "user_id" => ctx.user_id.as_deref(),
        "model" => ctx.model.as_deref(),
        "request_id" => ctx.request_id.as_deref(),
        "correlation_id" => ctx.correlation_id.as_deref(),
        "sandbox_profile" => ctx.sandbox_profile.as_deref(),
        _ => None,
    }
}

pub async fn handle(
    cmd: ContextCmd,
    config: &Config,
    config_path: Option<&std::path::Path>,
    store: Option<&str>,
) -> anyhow::Result<()> {
    let ctx = AgentCtx::from_env();
    let active_store = store.unwrap_or(&config.stores.default);

    if cmd.json {
        let agent_ctx = ctx.as_json();
        let out = json!({
            "agent_ctx": agent_ctx,
            "agent_ctx_present": !ctx.is_empty(),
            "active_store": active_store,
            "config_path": config_path.map(|p| p.display().to_string()),
            "default_agent_name": ctx.default_agent_name(),
            "default_agent_kind": ctx.default_agent_kind(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("Active store: {active_store}");
    if let Some(path) = config_path {
        println!("Config path:  {}", path.display());
    } else {
        println!("Config path:  (default — no --config provided)");
    }
    println!();

    if ctx.is_empty() {
        println!("AGENT_CTX_*: no context detected in environment.");
        return Ok(());
    }

    println!("AGENT_CTX_*:");
    for (env_key, field) in FIELDS {
        match field_value(&ctx, field) {
            Some(v) => println!("  {env_key:<32} = {v}"),
            None => println!("  {env_key:<32} = (unset)"),
        }
    }

    println!();
    if let Some(name) = ctx.default_agent_name() {
        println!("Derived agent.name fallback: {name}");
    }
    if let Some(kind) = ctx.default_agent_kind() {
        println!("Derived agent.kind fallback: {kind}");
    }

    Ok(())
}
