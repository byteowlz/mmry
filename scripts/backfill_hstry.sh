#!/usr/bin/env bash
# Backfill all hstry conversations into per-repo mmry stores.
# Store name = basename(workspace), sanitized.
# Skips non-repo workspaces (/tmp, $HOME, .pi/agent, empty).
set -uo pipefail

MMRY="${MMRY_BIN:-./target/release/mmry}"
LOG="${BACKFILL_LOG:-/tmp/mmry-backfill.log}"
STATE_DIR="${BACKFILL_STATE:-/tmp/mmry-backfill-state}"
mkdir -p "$STATE_DIR"

INGESTED_FILE="$STATE_DIR/ingested.txt"
touch "$INGESTED_FILE"

# Seed with the one session we manually ingested earlier.
grep -q '^019e3fab-bc7d-70bc-b17f-c3afbff6a003$' "$INGESTED_FILE" || \
  echo '019e3fab-bc7d-70bc-b17f-c3afbff6a003' >> "$INGESTED_FILE"

derive_store() {
  local ws="$1"
  case "$ws" in
    ""|"/"|"/tmp"|"/tmp/"*|"/home/wismut"|"/Users/tommyfalkowski"|"/home/wismut/.pi"|"/home/wismut/.pi/"*|"/Users/tommyfalkowski/.pi"|"/Users/tommyfalkowski/.pi/"*|"/home/wismut/.claude"|"/home/wismut/.claude/"*|"/Users/tommyfalkowski/dotfiles_stow"*)
      return 1
      ;;
  esac
  local base
  base=$(basename "$ws")
  base=$(echo "$base" | tr -c 'A-Za-z0-9_-' '_')
  base=$(echo "$base" | sed -E 's/^[_-]+//; s/[_-]+$//')
  base=${base:0:64}
  [ -z "$base" ] && return 1
  printf '%s' "$base"
}

TOTAL=0; INGESTED=0; SKIPPED=0; FAILED=0
BEFORE=""

while :; do
  if [ -z "$BEFORE" ]; then
    BATCH=$(hstry list --limit 2000 --json 2>/dev/null)
  else
    BATCH=$(hstry list --limit 2000 --before "$BEFORE" --json 2>/dev/null)
  fi
  COUNT=$(echo "$BATCH" | jq '.result | length' 2>/dev/null || echo 0)
  if [ "$COUNT" = "0" ] || [ -z "$COUNT" ]; then
    break
  fi
  echo "[batch] $COUNT sessions before=${BEFORE:-now}" | tee -a "$LOG"

  # Capture oldest created_at for next page boundary.
  NEW_BEFORE=$(echo "$BATCH" | jq -r '.result | map(.created_at) | min // empty')

  while IFS=$'\t' read -r conv_id external_id source_id workspace; do
    TOTAL=$((TOTAL+1))
    [ -z "$external_id" ] && external_id="$conv_id"

    if grep -qxF "$external_id" "$INGESTED_FILE"; then
      SKIPPED=$((SKIPPED+1))
      continue
    fi

    store=$(derive_store "$workspace") || {
      SKIPPED=$((SKIPPED+1))
      echo "skip workspace=$workspace conv=$conv_id" >> "$LOG"
      continue
    }

    if ! hstry --json show "$conv_id" 2>/dev/null | jq -e '.result' >/tmp/mmry-backfill-payload.json; then
      FAILED=$((FAILED+1))
      echo "fail show conv=$conv_id" >> "$LOG"
      continue
    fi

    if "$MMRY" --store "$store" add-conversation /tmp/mmry-backfill-payload.json >>"$LOG" 2>&1; then
      INGESTED=$((INGESTED+1))
      echo "$external_id" >> "$INGESTED_FILE"
      if (( INGESTED % 25 == 0 )); then
        echo "progress ingested=$INGESTED skipped=$SKIPPED failed=$FAILED total=$TOTAL"
      fi
    else
      FAILED=$((FAILED+1))
      echo "fail ingest conv=$conv_id store=$store" >> "$LOG"
    fi
  done < <(echo "$BATCH" | jq -r '.result[] | [.id, .external_id, .source_id, (.workspace // "")] | @tsv')

  if [ "$COUNT" -lt 2000 ] || [ -z "$NEW_BEFORE" ] || [ "$NEW_BEFORE" = "$BEFORE" ]; then
    break
  fi
  BEFORE="$NEW_BEFORE"
done

echo "DONE ingested=$INGESTED skipped=$SKIPPED failed=$FAILED total=$TOTAL" | tee -a "$LOG"
