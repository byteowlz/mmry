//! Regenerate the JSON schemas in `examples/` from the typed models.

use std::path::Path;

fn main() -> anyhow::Result<()> {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for (file, schema) in [
        (
            "config.schema.json",
            mmry_core::config::Config::schema_json()?,
        ),
        ("preview.schema.json", mmry_core::preview::schema_json()?),
        ("memory.schema.json", mmry_core::repos::entry_schema_json()?),
        ("cleanup.schema.json", mmry_core::cleanup::schema_json()?),
    ] {
        std::fs::write(examples.join(file), format!("{schema}\n"))?;
    }
    Ok(())
}
