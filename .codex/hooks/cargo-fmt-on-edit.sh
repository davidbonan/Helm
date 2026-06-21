#!/usr/bin/env bash
# PostToolUse hook: reformats Rust code after each edit (cargo fmt).
# Never blocks Claude — all errors are silent.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"

file=$(jq -r '.tool_input.file_path // empty' 2>/dev/null)
case "$file" in
  *.rs) ;;
  *) exit 0 ;;
esac

cargo fmt 2>/dev/null || true
