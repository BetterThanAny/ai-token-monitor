#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$PROJECT_ROOT"
codex mcp add gigabrain -- /bin/sh "$PROJECT_ROOT/.codex/actions/launch-gigabrain-mcp.sh"
codex mcp get gigabrain
