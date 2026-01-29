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

## Issue Tracking (trx)

```bash
trx ready              # Show unblocked issues
trx create "Title" -t task -p 2   # Create issue (types: bug/feature/task/epic/chore, priority: 0-4)
trx update <id> --status in_progress
trx close <id> -r "Done"
trx sync               # Commit .trx/ changes
```

Priorities: 0=critical, 1=high, 2=medium, 3=low, 4=backlog
