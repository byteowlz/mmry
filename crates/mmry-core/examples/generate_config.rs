//! Regenerate `examples/config.schema.json` from the typed config model.

use std::path::Path;

fn main() -> anyhow::Result<()> {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let schema = mmry_core::config::Config::schema_json()?;
    std::fs::write(examples.join("config.schema.json"), format!("{schema}\n"))?;
    Ok(())
}
