#!/usr/bin/env bash
# Alias used by the proposal: the effect scorecard lives in gate-q.sh.
set -euo pipefail
cd "$(dirname "$0")"
exec bash gate-q.sh
