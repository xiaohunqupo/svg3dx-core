#!/usr/bin/env bash
# Generate cpp/include/svg3dx.h from svg3dx-ffi-c via cbindgen.
# Usage:
#   ./scripts/generate-c-header.sh
#   ./scripts/generate-c-header.sh --check
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
export PATH="${HOME}/.cargo/bin:${PATH}"

CHECK=0
if [[ "${1:-}" == "--check" || "${1:-}" == "-Check" ]]; then
  CHECK=1
fi

if ! command -v cbindgen >/dev/null 2>&1; then
  echo "==> install cbindgen"
  cargo install cbindgen --locked
fi

CONFIG="$ROOT/crates/svg3dx-ffi-c/cbindgen.toml"
HEADER="$ROOT/cpp/include/svg3dx.h"
if [[ "$CHECK" -eq 1 ]]; then
  OUT="$(mktemp "${TMPDIR:-/tmp}/svg3dx-h.XXXXXX.h")"
  echo "==> cbindgen -> temp (check)"
else
  OUT="$HEADER"
  echo "==> cbindgen -> cpp/include/svg3dx.h"
fi

# Opaque Svg3dxResult / Svg3dxSession are declared in after_includes (Rust-only fields).
set +e
STDERR="$(cbindgen --config "$CONFIG" --crate svg3dx-ffi-c --output "$OUT" 2>&1 >/dev/null)"
CODE=$?
set -e
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  case "$line" in
    *"Can't find Svg3dxResult"*|*"Can't find Svg3dxSession"*) continue ;;
  esac
  echo "$line"
done <<<"$STDERR"

if [[ "$CODE" -ne 0 ]]; then
  [[ "$CHECK" -eq 1 && -f "$OUT" ]] && rm -f "$OUT"
  exit "$CODE"
fi

if [[ ! -f "$OUT" ]]; then
  echo "cbindgen produced no output" >&2
  exit 1
fi

if [[ "$CHECK" -eq 1 ]]; then
  if [[ ! -f "$HEADER" ]]; then
    rm -f "$OUT"
    echo "Missing checked-in header: $HEADER (run without --check to generate)" >&2
    exit 1
  fi
  # Normalize CRLF so Windows-checked-in headers compare cleanly on Linux.
  EXPECTED="$(tr -d '\r' <"$OUT")"
  ACTUAL="$(tr -d '\r' <"$HEADER")"
  rm -f "$OUT"
  if [[ "$EXPECTED" != "$ACTUAL" ]]; then
    echo "cpp/include/svg3dx.h is stale. Run: ./scripts/generate-c-header.sh" >&2
    exit 1
  fi
  echo "Header is up to date."
else
  echo "Wrote $HEADER"
fi
