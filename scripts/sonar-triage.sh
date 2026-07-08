#!/usr/bin/env bash

set -euo pipefail
umask 077

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=${TRIAGE_REPO_ROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}
ENV_FILE=${TRIAGE_ENV_FILE:-"$HOME/.config/bsl-sonar-triage/env"}
STATE_DIR=${TRIAGE_STATE_DIR:-"$HOME/.local/state/bsl-sonar-triage"}
PROCESSED_LEDGER=${TRIAGE_PROCESSED_LEDGER:-"$STATE_DIR/processed.ndjson"}
PROMPT_FILE=${TRIAGE_PROMPT_FILE:-"$SCRIPT_DIR/sonar-triage-prompt.md"}
TRIAGE_LABEL=${TRIAGE_LABEL:-sonar-triage}
TRIAGE_SINCE=${TRIAGE_SINCE:-2025-01-01}
TRIAGE_MAX_ISSUES=${TRIAGE_MAX_ISSUES:-50}
TRIAGE_RESOLUTIONS=${TRIAGE_RESOLUTIONS:-FALSE-POSITIVE}
TRIAGE_PROJECTS=${TRIAGE_PROJECTS:-}
TRIAGE_GITLAB_REPO=${TRIAGE_GITLAB_REPO:-}
TRIAGE_SKIP_OPENCODE=${TRIAGE_SKIP_OPENCODE:-0}
TRIAGE_SRC_ROOTS=${TRIAGE_SRC_ROOTS:-"$HOME/src $HOME/src/pt"}
TRIAGE_PROJECT_MAP=${TRIAGE_PROJECT_MAP:-"$HOME/.config/bsl-sonar-triage/projects.map"}
TRIAGE_REFRESH_CLONES=${TRIAGE_REFRESH_CLONES:-0}
TRIAGE_INCLUDE_SNIPPETS=${TRIAGE_INCLUDE_SNIPPETS:-0}
if [[ "${TRIAGE_SKIP_SNIPPETS:-}" == 0 ]]; then
  TRIAGE_INCLUDE_SNIPPETS=1
elif [[ "${TRIAGE_SKIP_SNIPPETS:-}" == 1 ]]; then
  TRIAGE_INCLUDE_SNIPPETS=0
fi

CURL_BIN=${CURL_BIN:-curl}
GLAB_BIN=${GLAB_BIN:-glab}
JQ_BIN=${JQ_BIN:-jq}
OPENCODE_BIN=${OPENCODE_BIN:-opencode}

RUN_ID=""

source "$SCRIPT_DIR/sonar-triage-common.sh"
source "$SCRIPT_DIR/sonar-triage-collect.sh"
source "$SCRIPT_DIR/sonar-triage-analyze.sh"
source "$SCRIPT_DIR/sonar-triage-gitlab.sh"

usage() {
  cat <<'EOF'
Usage: scripts/sonar-triage.sh <command> [options]

Commands:
  preflight                 Check local tools and auth without reading Sonar.
  discover                  Read Sonar projects and write a run manifest.
  collect                   Collect closed/resolved Sonar issues into a run.
  analyze --run-id ID       Ask opencode to classify collected issues.
  plan --run-id ID          Build GitLab create/note actions via glab reads.
  apply --run-id ID         Dry-run planned GitLab actions.
  apply --run-id ID --live --confirm-live
                            Execute planned GitLab writes and advance state.
  run-dry                   Run collect -> analyze -> plan -> dry apply.

Options:
  --since YYYY-MM-DD        Closed issue lower bound. Default: TRIAGE_SINCE or 2025-01-01.
  --max N                   Max issues to collect across all servers. Default: 50. Use 0 for no cap.
  --run-id ID               Existing run id for analyze/plan/apply.
  --include-snippets        Store downstream source snippets in run artifacts and GitLab bodies.
  --refresh-clones          git pull --ff-only clean local clones before analyze.
  --live                    Allow apply to write via glab.
  --confirm-live            Required together with --live.

Environment:
  ~/.config/bsl-sonar-triage/env should define SONAR_*_URL and SONAR_*_TOKEN.
  Optional TRIAGE_PROJECTS format: runsystems:projectKey,primeit:projectKey
  Optional TRIAGE_GITLAB_REPO forces glab --repo; otherwise current repo is used.
  Optional TRIAGE_RESOLUTIONS comma list. Default: FALSE-POSITIVE. FIXED closures
    are mostly style compliance, not analyzer bugs; add them only deliberately.
  Optional TRIAGE_SRC_ROOTS space list of parents searched for a <project> clone.
    Default: "~/src ~/src/pt". A cloned project lets opencode read the real code.
  Optional TRIAGE_PROJECT_MAP file with `projectKey=/path` lines for odd layouts.

Idempotency:
  A live apply records every triaged issue (create and skip alike) into a local
  ledger; later collect runs drop those issues before analyze, so opencode never
  re-runs on an already-processed Sonar issue. Dry runs never touch the ledger.
EOF
}

parse_common_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --since) TRIAGE_SINCE=${2:?--since requires a value}; shift 2 ;;
      --max) TRIAGE_MAX_ISSUES=${2:?--max requires a value}; shift 2 ;;
      --run-id) RUN_ID=${2:?--run-id requires a value}; shift 2 ;;
      --include-snippets) TRIAGE_INCLUDE_SNIPPETS=1; shift ;;
      --refresh-clones) TRIAGE_REFRESH_CLONES=1; shift ;;
      *) die "unknown option: $1" ;;
    esac
  done
}

parse_apply_args() {
  LIVE_APPLY=0
  CONFIRM_LIVE=0
  local rest=()
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --live) LIVE_APPLY=1; shift ;;
      --confirm-live) CONFIRM_LIVE=1; shift ;;
      *) rest+=("$1"); shift ;;
    esac
  done
  parse_common_args "${rest[@]}"
}

main() {
  local command=${1:-}
  [[ -n "$command" ]] || { usage; exit 1; }
  shift || true
  ensure_state_dirs
  case "$command" in
    preflight) parse_common_args "$@"; preflight ;;
    discover) parse_common_args "$@"; discover ;;
    collect) parse_common_args "$@"; collect ;;
    analyze) parse_common_args "$@"; analyze ;;
    plan) parse_common_args "$@"; plan_gitlab ;;
    apply)
      parse_apply_args "$@"
      apply_actions "$LIVE_APPLY" "$CONFIRM_LIVE"
      ;;
    run-dry)
      parse_common_args "$@"
      RUN_ID=${RUN_ID:-$(new_run_id)}
      collect
      analyze
      plan_gitlab
      apply_actions 0 0
      ;;
    -h|--help|help) usage ;;
    *) usage; exit 1 ;;
  esac
}

main "$@"
