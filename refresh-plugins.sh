#!/bin/bash
set -euo pipefail

# Discoverable alias for the reload-installed workflow. Operates on whatever
# this repo is currently checked out to -- run 'git pull' yourself first if
# you want main; leaving that step manual also means this works unmodified
# for testing a feature branch or a pinned older commit.
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$REPO_DIR/setup-plugins.sh" --reload
