#!/usr/bin/env bash
set -euo pipefail

SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC_SCHEMA="$SRC_DIR/examples/config.schema.json"
DEST_DIR="$SRC_DIR/../schemas/mmry"
DEST_SCHEMA="$DEST_DIR/mmry.config.schema.json"

if [ ! -f "$SRC_SCHEMA" ]; then
  echo "Source schema not found: $SRC_SCHEMA" >&2
  exit 1
fi

mkdir -p "$DEST_DIR"
cp "$SRC_SCHEMA" "$DEST_SCHEMA"
echo "Copied schema to $DEST_SCHEMA"

cd "$DEST_DIR"
git add .
git commit -m "feat: updated mmry schema"
git push
echo "Committed and pushed schema changes"
