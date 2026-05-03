#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
GIGABRAIN_CONFIG="${GIGABRAIN_CONFIG:-$HOME/.gigabrain/config.json}"
PACKAGE_ROOT_HINT=''
PACKAGE_SPEC='@legendaryvibecoder/gigabrain@0.6.1'

run_gigabrain_cli() {
  local tool="$1"
  local script_rel="$2"
  shift 2

  if [ -x "$PROJECT_ROOT/node_modules/.bin/$tool" ]; then
    "$PROJECT_ROOT/node_modules/.bin/$tool" "$@"
    return
  fi

  if command -v "$tool" >/dev/null 2>&1; then
    "$(command -v "$tool")" "$@"
    return
  fi

  if command -v npx >/dev/null 2>&1 && npx --no-install "$tool" --help >/dev/null 2>&1; then
    npx --no-install "$tool" "$@"
    return
  fi

  if [ -n "$PACKAGE_ROOT_HINT" ] && [ -f "$PACKAGE_ROOT_HINT/$script_rel" ] && command -v node >/dev/null 2>&1; then
    node "$PACKAGE_ROOT_HINT/$script_rel" "$@"
    return
  fi

  if [ -n "$PACKAGE_SPEC" ] && command -v npx >/dev/null 2>&1; then
    npx --yes --package "$PACKAGE_SPEC" "$tool" "$@"
    return
  fi

  echo "Gigabrain helper could not find $tool." >&2
  echo "Tried repo-local node_modules/.bin, command -v, npx --no-install, a stable setup-time source hint, and npx --package $PACKAGE_SPEC." >&2
  echo "Run npm install @legendaryvibecoder/gigabrain in this repo or rerun the Gigabrain setup script to refresh the generated helpers." >&2
  return 1
}

cd "$PROJECT_ROOT"
if [ -f package.json ]; then
  npm install
fi
NODE_WARNING_FLAGS="--no-warnings=ExperimentalWarning"
export NODE_OPTIONS="${NODE_WARNING_FLAGS}${NODE_OPTIONS:+ ${NODE_OPTIONS}}"
run_gigabrain_cli gigabrain-codex-setup scripts/gigabrain-codex-setup.js --project-root "$PROJECT_ROOT" --config "$GIGABRAIN_CONFIG" --store-mode 'global'
"$PROJECT_ROOT/.codex/actions/verify-gigabrain.sh"
