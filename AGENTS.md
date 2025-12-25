# AGENTS.md

This file provides guidance to AI Agents when working with code in this repository.

## General

**NEVER PUBLISH TO PACKAGE REPOSITORIES WITHOUT EXPLICIT PERMISSION**: Under no circumstances should you publish any packages to cargo/homebrew/npm or any other public registry without explicit permission from the user. This is a critical security and trust boundary that must never be crossed.

**No Backwards Compatibility**: We never care about backwards compatibility. We prioritize clean, modern code and user experience over maintaining legacy support. Breaking changes are acceptable and expected as the project evolves. This includes removing deprecated code, changing APIs freely, and not supporting legacy formats or approaches.

**No "Modern" or Version Suffixes**: When refactoring, never use names like "Modern", "New", "V2", etc. Simply refactor the existing things in place. If we are doing a refactor, we want to replace the old implementation completely, not create parallel versions. Use the idiomatic name that the API should have.

**Cross-Platform compatibility**: This project targets Windows 11, Linux and macOS 14.0 (Sonoma) and later. Do not add availability checks for macOS versions below 14.0.

**File Headers**: Use minimal file headers without author attribution or creation dates:

- Omit "Created by" comments and dates to keep headers clean and focused

## Rust

In the crate folder where the rust code lives:

- Crate names are prefixed with mmry-. For example, the core folder's crate is named mmry-core
- When using format! and you can inline variables into {}, always do that.
- Install any commands the repo relies on (for example just, rg, or cargo-insta) if they aren't already available before running instructions here.
- Always collapse if statements per <https://rust-lang.github.io/rust-clippy/master/index.html#collapsible_if>
- Always inline format! args when possible per <https://rust-lang.github.io/rust-clippy/master/index.html#uninlined_format_args>
- Use method references over closures when possible per <https://rust-lang.github.io/rust-clippy/master/index.html#redundant_closure_for_method_calls>
- When writing tests, prefer comparing the equality of entire objects over fields one by one.

Run just fmt automatically after making Rust code changes; do not ask for approval to run it. Before finalizing a change to mmry, run just fix -p <project> (in the root directory) to fix any linter issues in the code. Prefer scoping with -p to avoid slow workspace‑wide Clippy builds; only run just fix without -p if you changed shared crates. Additionally, run the tests:

- Run the test for the specific project that was changed. For example, if changes were made in mmry-cli, run cargo test -p mmry-cli.
- Once those pass, if any changes were made in common, core, or protocol, run the complete test suite with cargo test --all-features. When running interactively, ask the user before running just fix to finalize. just fmt does not require approval. project-specific or individual tests can be run without asking the user, but do ask the user before running the complete test suite.

## Debugging

**What to Look For:**

1. **Common Agent Mistakes**:
   - Missing required parameters or incorrect parameter usage
   - Misunderstanding of command syntax or options
   - Attempting unsupported operations
   - Confusion about tool capabilities or limitations

2. **Actual Bugs**:
   - Crashes, errors, or unexpected behavior
   - Missing functionality that should exist
   - Performance issues or timeouts
   - Inconsistent behavior across similar commands

3. **UX Improvements**:
   - Unclear error messages that could be more helpful
   - Missing hints or suggestions when agents make mistakes
   - Opportunities to add guardrails or validation
   - Places where agents get stuck in loops or retry patterns

4. **Missing Features**:
   - Common operations that require multiple steps but could be simplified
   - Patterns where agents work around limitations
   - Frequently attempted unsupported commands

**How to Analyze:**

1. Read through the entire log systematically
2. Identify patterns of confusion or repeated attempts
3. Note any error messages that could be clearer
4. Look for places where the agent had to guess or try multiple approaches
5. Consider what helpful messages or features would have prevented issues

**Output Format:**

- List specific bugs found with reproduction steps
- Suggest concrete improvements to error messages
- Recommend new features or commands based on agent behavior
- Propose additions to system/tool prompts to guide future agents
- Prioritize fixes by impact on agent experience

# important-instruction-reminders

Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (\*.md) or README files. Only create documentation files if explicitly requested by the User.

Awesome CLIs (adhere to these rules when applicable):

Design & UX
• Prefer subcommands for verbs (tool add, tool list) and keep each one focused.
• Be composable: read from stdin, write to stdout, errors to stderr, use clear exit codes.
• Ship sensible defaults; require as few flags as possible.
• Destructive ops: provide --dry-run, prompt only on TTY, support --yes/--force.
• Errors that teach: show cause + fix, suggest near-miss flags/values.

Help & Discoverability
• Fast -h/--help with one-liners; rich help <subcmd> with examples.
• --version prints semver; note deprecations.
• Offer completions generator (tool completions bash|zsh|fish|powershell) and man page/README snippet.

I/O & Formats
• Human output by default; machine output via --json/--yaml (stable schema).
• Respect NO_COLOR/FORCE_COLOR; auto-disable color when not a TTY.
• Verbosity controls: -q/--quiet, stackable -v, --debug, --trace.
• Progress bars/spinners only on TTY; provide --no-progress.

Config & Environment
• Clear precedence: flags > env > config file.
• Use XDG dirs on Unix; sensible Windows paths; --config PATH.
• config init, config show (effective config with sources).

Performance & Reliability
• Fast startup; lazy-load heavy parts.
• Timeouts & retries where networked; --parallel N, --timeout.
• Idempotent behavior; --keep-going and --fail-fast.

Security & Privacy
• Never print secrets by default; mask in logs (--redact).
• Support secrets via env/secret stores; avoid writing to shell history (suggest ENV_VAR=… tool …).

Packaging & Distribution
• Cross-platform builds; reproducible releases; signed artifacts.
• Easy install paths (Homebrew/Scoop/AUR/pkg managers) or single static binary.
• Optional self-update and gentle version-checks (opt-out env).

Observability & Testing
• Logs to stderr with timestamps on --debug/--trace.
• --profile to show timing breakdowns.
• Golden tests for text output; schema tests for JSON; fuzz your flag parser.

Accessibility & Internationalization
• Avoid ASCII art; provide --no-emoji.
• High-contrast, monochrome-friendly output.
• English fallback; locale-aware messages (if you localize).

Standard Flag Kit (copy these across subcommands)

-h, --help · -V, --version · -q, --quiet · -v (stackable) · --debug · --trace · --json/--yaml · --no-color (and honor NO_COLOR) · --dry-run · -y, --yes/--force · --config PATH · --no-progress · --timeout SEC · --parallel N

Bonus power moves
• Context-aware help (suggest flags based on partial input).
• Interactive wizards (init, login) gated behind TTY; pure flags otherwise.
• Plugin system: discover commands from $PATH like tool-foo → tool foo.
• Generate shell completions dynamically from your parser so help & completion never drift.

## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Git-friendly: Auto-syncs to JSONL for version control
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Quick Start

**Check for ready work:**

```bash
bd ready --json
```

**Create new issues:**

```bash
bd create "Issue title" -t bug|feature|task -p 0-4 --json
bd create "Issue title" -p 1 --deps discovered-from:bd-123 --json
```

**Claim and update:**

```bash
bd update bd-42 --status in_progress --json
bd update bd-42 --priority 1 --json
```

**Complete work:**

```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task**: `bd update <id> --status in_progress`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`
6. **Commit together**: Always commit the `.beads/issues.jsonl` file together with the code changes so issue state stays in sync with code state

### Auto-Sync

bd automatically syncs with git:

- Exports to `.beads/issues.jsonl` after changes (5s debounce)
- Imports from JSONL when newer (e.g., after `git pull`)
- No manual export/import needed!

### MCP Server (Recommended)

If using Claude or MCP-compatible clients, install the beads MCP server:

```bash
pip install beads-mcp
```

Add to MCP config (e.g., `~/.config/claude/config.json`):

```json
{
  "beads": {
    "command": "beads-mcp",
    "args": []
  }
}
```

Then use `mcp__beads__*` functions instead of CLI commands.

### Managing AI-Generated Planning Documents

AI assistants often create planning and design documents during development:

- PLAN.md, IMPLEMENTATION.md, ARCHITECTURE.md
- DESIGN.md, CODEBASE_SUMMARY.md, INTEGRATION_PLAN.md
- TESTING_GUIDE.md, TECHNICAL_DESIGN.md, and similar files

**Best Practice: Use a dedicated directory for these ephemeral files**

**Recommended approach:**

- Create a `history/` directory in the project root
- Store ALL AI-generated planning/design docs in `history/`
- Keep the repository root clean and focused on permanent project files
- Only access `history/` when explicitly asked to review past planning

**Example .gitignore entry (optional):**

```
# AI planning documents (ephemeral)
history/
```

**Benefits:**

- ✅ Clean repository root
- ✅ Clear separation between ephemeral and permanent documentation
- ✅ Easy to exclude from version control if desired
- ✅ Preserves planning history for archeological research
- ✅ Reduces noise when browsing the project

### Important Rules

- ✅ Use bd for ALL task tracking
- ✅ Always use `--json` flag for programmatic use
- ✅ Link discovered work with `discovered-from` dependencies
- ✅ Check `bd ready` before asking "what should I work on?"
- ✅ Store AI planning docs in `history/` directory
- ❌ Do NOT create markdown TODO lists
- ❌ Do NOT use external issue trackers
- ❌ Do NOT duplicate tracking systems
- ❌ Do NOT clutter repo root with planning documents

For more details, see README.md and QUICKSTART.md.

<!-- bv-agent-instructions-v1 -->

---

## Beads Workflow Integration

This project uses [beads_viewer](https://github.com/Dicklesworthstone/beads_viewer) for issue tracking. Issues are stored in `.beads/` and tracked in git.

### Essential Commands

```bash
# View issues (launches TUI - avoid in automated sessions)
bv

# CLI commands for agents (use these instead)
bd ready              # Show issues ready to work (no blockers)
bd list --status=open # All open issues
bd show <id>          # Full issue details with dependencies
bd create --title="..." --type=task --priority=2
bd update <id> --status=in_progress
bd close <id> --reason="Completed"
bd close <id1> <id2>  # Close multiple issues at once
bd sync               # Commit and push changes
```

### Workflow Pattern

1. **Start**: Run `bd ready` to find actionable work
2. **Claim**: Use `bd update <id> --status=in_progress`
3. **Work**: Implement the task
4. **Complete**: Use `bd close <id>`
5. **Sync**: Always run `bd sync` at session end

### Key Concepts

- **Dependencies**: Issues can block other issues. `bd ready` shows only unblocked work.
- **Priority**: P0=critical, P1=high, P2=medium, P3=low, P4=backlog (use numbers, not words)
- **Types**: task, bug, feature, epic, question, docs
- **Blocking**: `bd dep add <issue> <depends-on>` to add dependencies

### Session Protocol

**Before ending any session, run this checklist:**

```bash
git status              # Check what changed
git add <files>         # Stage code changes
bd sync                 # Commit beads changes
git commit -m "..."     # Commit code
bd sync                 # Commit any new beads changes
git push                # Push to remote
```

### Best Practices

- Check `bd ready` at session start to find available work
- Update status as you work (in_progress → closed)
- Create new issues with `bd create` when you discover tasks
- Use descriptive titles and set appropriate priority/type
- Always `bd sync` before ending session

<!-- end-bv-agent-instructions -->
