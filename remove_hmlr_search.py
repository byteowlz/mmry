#!/usr/bin/env python3
"""Remove HMLR types and search function from search/mod.rs"""

import re

def main():
    with open("crates/mmry-core/src/search/mod.rs", "r") as f:
        content = f.read()

    # Remove imports
    content = re.sub(r'use crate::agents::BridgeBlock;\n', '', content)
    content = re.sub(r'use crate::agents::FactRecord;\n', '', content)

    # Remove HmlrSearchOptions struct (from comment to closing brace)
    pattern = r'/// Options for HMLR-enhanced search\n#\[derive\(Debug, Clone, Default\)\]\npub struct HmlrSearchOptions \{[^}]+\}\n'
    content = re.sub(pattern, '', content, flags=re.DOTALL)

    # Remove InactiveBlockStrategy enum
    pattern = r'#\[derive\(Debug, Clone, Default, PartialEq\)\]\npub enum InactiveBlockStrategy \{[^}]+\}\n'
    content = re.sub(pattern, '', content, flags=re.DOTALL)

    # Remove HmlrSearchResult struct
    pattern = r'/// HMLR-enhanced search result with facts and bridge blocks\n#\[derive\(Debug, Clone\)\]\npub struct HmlrSearchResult \{[^}]+\}\n'
    content = re.sub(pattern, '', content, flags=re.DOTALL)

    # Find and remove search_with_hmlr function
    # This is tricky - need to find "pub async fn search_with_hmlr" and remove until next "async fn" or end of impl block
    start_marker = "    /// Search with HMLR enrichments"
    start_idx = content.find(start_marker)

    if start_idx != -1:
        # Find the next method after search_with_hmlr (should be vector_candidates or similar)
        next_method = content.find("\n    async fn vector_candidates", start_idx)
        if next_method == -1:
            next_method = content.find("\n}\n", start_idx)  # End of impl block

        if next_method != -1:
            # Remove from start_marker to next_method
            content = content[:start_idx] + content[next_method:]
            print(f"Removed search_with_hmlr function")

    # Clean up double blank lines
    while "\n\n\n" in content:
        content = content.replace("\n\n\n", "\n\n")

    with open("crates/mmry-core/src/search/mod.rs", "w") as f:
        f.write(content)

    print("Done cleaning search/mod.rs")

if __name__ == "__main__":
    main()
