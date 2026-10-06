# Syncing between machines

Opt-in: the store becomes a git repository with a remote you choose. Authentication is whatever git already uses (ssh keys, credential helpers); mmry never prompts or stores credentials.

```bash
mmry sync init --remote git@github.com:you/mmry-state.git   # also merges an existing remote
mmry sync                          # commit, pull, push
mmry sync status [--json]          # remote, pending commits, last pull/push, last error, auto flags
mmry sync pull | push
mmry sync auto                     # show auto flags
mmry sync auto --on                # pull at session start, commit + push after every write
mmry sync auto --commit --push=false   # or set one step at a time
mmry sync auto --off
```

```toml
[sync]
auto_pull = true      # at session start (mmry preview)
auto_commit = true    # after every write
auto_push = true      # after an automatic commit
timeout_secs = 10
```

Ledgers merge with `merge=union` (written to `.gitattributes`): events are append-only lines with unique ids, so keeping both sides is correct, and concurrent edits of one memory show up as contested. Only `general/`, `repos/` and the two rule files are synced; everything else in the store (`local/` with checkout paths and sync status, files from other tools or older mmry versions) stays machine-local. Files that older versions committed are untracked on the next sync, and conflicts in them or in the rule files are resolved automatically. Offline or rejected pushes keep everything committed locally and report pending commits; a rejected push pulls once and retries. mmry never force-pushes or resets; any other conflict aborts the merge and is reported. Use a private remote: memories, `why`, sources and provenance (`agent_ctx`) are all in there, and `mmry rm` does not erase history.
