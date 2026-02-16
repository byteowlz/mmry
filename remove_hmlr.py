#!/usr/bin/env python3
"""Remove HMLR dead code from database/operations.rs"""

FUNCTIONS_TO_REMOVE = [
    "bridge_block_from_row",
    "upsert_bridge_block", 
    "update_bridge_block_embedding",
    "get_active_blocks_with_embeddings",
    "list_bridge_blocks_by_span",
    "list_bridge_blocks",
    "close_inactive_bridge_blocks",
    "count_bridge_blocks",
    "list_all_bridge_blocks",
    "get_recent_bridge_blocks_for_agent",
    "get_bridge_block",
    "get_bridge_block_by_span",
    "update_memory_bridge_block",
    "get_memories_by_bridge_block",
    "get_all_memories_by_bridge_block",
    "upsert_fact",
    "search_facts",
    "get_facts_for_memory",
    "list_recent_facts",
    "list_all_facts",
    "get_fact",
    "get_facts_by_category",
    "set_user_profile",
    "get_user_profile",
]

# These patterns indicate the end of a function
END_MARKERS = [
    "\npub async fn ",
    "\npub fn ",
    "\npub(crate) async fn ",
    "\npub(crate) fn ",
    "\nfn ",
    "\nasync fn ",
    "\n#[cfg(test)]",
]

def find_function_bounds(content, func_name):
    """Find start and end of a function definition"""
    search = f"pub async fn {func_name}("
    start = content.find(search)
    if start == -1:
        search = f"pub fn {func_name}("
        start = content.find(search)
    if start == -1:
        search = f"pub(crate) async fn {func_name}("
        start = content.find(search)
    if start == -1:
        search = f"pub(crate) fn {func_name}("
        start = content.find(search)
    if start == -1:
        search = f"fn {func_name}("
        start = content.find(search)

    if start == -1:
        return None

    # Find end of function - look for the next function or end of file
    # Skip past function signature to body
    brace_start = content.find("{", start)
    if brace_start == -1:
        return None

    # Count braces to find matching end
    depth = 1
    i = brace_start + 1
    in_string = False
    string_char = None
    escaped = False

    while i < len(content) and depth > 0:
        c = content[i]

        if escaped:
            escaped = False
            i += 1
            continue

        if c == '\\' and in_string:
            escaped = True
            i += 1
            continue

        if c in ('"', "'") and not in_string:
            in_string = True
            string_char = c
        elif c == string_char and in_string:
            in_string = False
            string_char = None
        elif not in_string:
            if c == '{':
                depth += 1
            elif c == '}':
                depth -= 1

        i += 1

    return (start, i)

def main():
    with open("crates/mmry-core/src/database/operations.rs", "r") as f:
        content = f.read()

    # Find and remove each function
    removed = []
    for func_name in FUNCTIONS_TO_REMOVE:
        bounds = find_function_bounds(content, func_name)
        if bounds:
            start, end = bounds
            # Check if it's preceded by a doc comment
            # Look backwards for ///
            search_start = start
            while search_start > 0:
                line_start = content.rfind("\n", 0, search_start)
                if line_start == -1:
                    break
                line = content[line_start:search_start].strip()
                if line.startswith("///") or line.startswith("//") or line == "":
                    search_start = line_start
                else:
                    break

            # Include preceding blank lines
            while search_start > 0 and content[search_start-1] == "\n":
                search_start -= 1

            removed.append((search_start, end, func_name))
            print(f"Found {func_name} at lines {content[:search_start].count(chr(10))+1}-{content[:end].count(chr(10))+1}")

    # Sort by position in reverse order so we can remove from end to start
    removed.sort(key=lambda x: x[0], reverse=True)

    # Remove each function
    for start, end, func_name in removed:
        content = content[:start] + content[end:]
        print(f"Removed {func_name}")

    # Clean up double blank lines
    while "\n\n\n" in content:
        content = content.replace("\n\n\n", "\n\n")

    with open("crates/mmry-core/src/database/operations.rs", "w") as f:
        f.write(content)

    print(f"\nRemoved {len(removed)} functions")

if __name__ == "__main__":
    main()
