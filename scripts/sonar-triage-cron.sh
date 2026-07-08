#!/usr/bin/env bash

# Scheduled sonar-triage run: collect -> analyze -> plan -> live apply.
# The processed-ledger records every triaged issue, so each run only spends
# opencode on issues that closed since the previous successful run.

set -euo pipefail

export PATH="$HOME/.opencode/bin:/usr/local/bin:/usr/bin:/bin:$PATH"

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
TRIAGE="$SCRIPT_DIR/sonar-triage.sh"
SINCE=${TRIAGE_SINCE:-2025-01-01}

# Only explicit false positives feed the GitLab backlog automatically; WONTFIX
# and FIXED closures are reviewed out of band, not auto-filed.
export TRIAGE_RESOLUTIONS=${TRIAGE_RESOLUTIONS:-FALSE-POSITIVE}
LOG_DIR=${TRIAGE_LOG_DIR:-"$HOME/.local/state/bsl-sonar-triage/logs"}
mkdir -p "$LOG_DIR"

RUN_ID="scheduled-$(date -u '+%Y%m%dT%H%M%SZ')"
LOG_FILE="$LOG_DIR/$RUN_ID.log"
exec >>"$LOG_FILE" 2>&1

echo "[cron] run $RUN_ID start $(date -u '+%FT%TZ')"
"$TRIAGE" preflight
"$TRIAGE" collect --run-id "$RUN_ID" --since "$SINCE" --max 0
"$TRIAGE" analyze --run-id "$RUN_ID"
"$TRIAGE" plan --run-id "$RUN_ID"
"$TRIAGE" apply --run-id "$RUN_ID" --live --confirm-live
echo "[cron] run $RUN_ID done $(date -u '+%FT%TZ')"

find "$LOG_DIR" -name 'scheduled-*.log' -mtime +30 -delete 2>/dev/null || true
