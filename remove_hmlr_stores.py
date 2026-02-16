#!/usr/bin/env python3
"""Remove HMLR code from stores.rs"""

import re

def main():
    with open("crates/mmry-core/src/stores.rs", "r") as f:
        content = f.read()

    # Remove FactWithStore struct
    pattern = r'/// A fact with its source store name\n#\[derive\(Debug, Clone, serde::Serialize, serde::Deserialize\)\]\npub struct FactWithStore \{[^}]+\}\n'
    content = re.sub(pattern, '\n', content, flags=re.DOTALL)

    # Remove ExportedFact struct
    pattern = r'/// Exported fact record[^}]+\}[^}]+\}\n'
    content = re.sub(pattern, '\n', content, flags=re.DOTALL)

    # Remove ExportedBridgeBlock struct
    pattern = r'/// Exported bridge block[^}]+open_loops[^}]+decisions_made[^}]+memory_ids[^}]+\}\n'
    content = re.sub(pattern, '\n', content, flags=re.DOTALL)

    # Remove ExportedEntity struct
    pattern = r'/// Exported entity[^}]+metadata[^}]+\}\n'
    content = re.sub(pattern, '\n', content, flags=re.DOTALL)

    # Remove ExportedRelationship struct
    pattern = r'/// Exported relationship[^}]+strength[^}]+\}\n'
    content = re.sub(pattern, '\n', content, flags=re.DOTALL)

    # Remove ExportedMemoryEntity struct
    pattern = r'/// Memory-entity link[^}]+entity_id[^}]+\}\n'
    content = re.sub(pattern, '\n', content, flags=re.DOTALL)

    # Remove ExportedHmlr struct
    pattern = r'/// HMLR[^}]+ExportedHmlr \{[^}]+facts[^}]+bridge_blocks[^}]+entities[^}]+relationships[^}]+memory_entities[^}]+\}[^}]+\}\n'
    content = re.sub(pattern, '\n', content, flags=re.DOTALL)

    # Remove hmlr field from ExportResult
    pattern = r'    /// HMLR[^}]+pub hmlr: Option<ExportedHmlr>,\n'
    content = re.sub(pattern, '\n', content)

    # Remove version field and default_version function
    pattern = r'    /// Export format[^}]+version: u32,\n'
    content = re.sub(pattern, '    pub version: u32,\n', content)
    content = re.sub(r'fn default_version\(\) -> u32 \{[^}]+\}\n', '', content)

    # Remove include_hmlr parameters and related code
    content = re.sub(r'include_hmlr: bool,', '', content)
    content = re.sub(r'export_store_to_json_with_options\([^,]+,[^,]+,[^)]+\)', 'export_store_to_json(config, store_name)', content)

    # Remove list_all_facts function
    start = content.find("/// List facts from all stores")
    if start != -1:
        end_marker = content.find("/// Export memories from a single store", start)
        if end_marker != -1:
            content = content[:start] + content[end_marker:]

    # Clean up double blank lines
    while "\n\n\n" in content:
        content = content.replace("\n\n\n", "\n\n")

    with open("crates/mmry-core/src/stores.rs", "w") as f:
        f.write(content)

    print("Done cleaning stores.rs")

if __name__ == "__main__":
    main()
