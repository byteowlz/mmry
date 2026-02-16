#!/usr/bin/env python3
"""Clean up HMLR code from stores.rs"""

def main():
    with open("crates/mmry-core/src/stores.rs", "r") as f:
        content = f.read()

    # Find and remove ExportedFact struct (line 358-372)
    lines = content.split('\n')
    
    # Find the line numbers for each section to remove
    to_remove = []
    
    # Find structs and functions to remove
    for i, line in enumerate(lines):
        if any(x in line for x in [
            "pub struct ExportedFact",
            "pub struct ExportedBridgeBlock", 
            "pub struct ExportedEntity",
            "pub struct ExportedRelationship",
            "pub struct ExportedMemoryEntity",
            "pub struct ExportedHmlr",
            "async fn export_hmlr_data",
        ]):
            to_remove.append(i)
    
    print(f"Found {len(to_remove)} items to start removing")
    
    # For each starting line, find where it ends (next struct/fn or blank line)
    ranges = []
    for start in to_remove:
        # Find the end - next } on its own line or similar
        end = start + 1
        brace_count = 0
        in_struct = False
        
        for j in range(start, min(len(lines), start + 100)):
            line = lines[j]
            if '{' in line and not in_struct:
                in_struct = True
            if in_struct:
                brace_count += line.count('{')
                brace_count -= line.count('}')
                if brace_count == 0 and j > start:
                    end = j
                    break
        else:
            end = min(start + 50, len(lines) - 1)
        
        ranges.append((start, end))
    
    # Merge overlapping ranges
    ranges.sort()
    merged = []
    for r in ranges:
        if merged and r[0] <= merged[-1][1] + 1:
            merged[-1] = (merged[-1][0], max(merged[-1][1], r[1]))
        else:
            merged.append(r)
    
    print(f"Merged into {len(merged)} ranges: {merged}")

if __name__ == "__main__":
    main()
