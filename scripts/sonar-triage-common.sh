log() {
  printf '[sonar-triage] %s\n' "$*" >&2
}

die() {
  log "ERROR: $*"
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing command: $1"
}

load_env() {
  [[ -f "$ENV_FILE" ]] || die "env file not found: $ENV_FILE"
  unset SONAR_RUNSYSTEMS_TOKEN SONAR_PRIMEIT_TOKEN
  source "$ENV_FILE"
  export -n SONAR_RUNSYSTEMS_TOKEN SONAR_PRIMEIT_TOKEN
}

server_url() {
  case "$1" in
    runsystems) printf '%s' "${SONAR_RUNSYSTEMS_URL:-https://sonar.runsystems.ru}" ;;
    primeit) printf '%s' "${SONAR_PRIMEIT_URL:-https://sonar.prime-it.ru}" ;;
    *) die "unknown Sonar server: $1" ;;
  esac
}

server_token() {
  case "$1" in
    runsystems) printf '%s' "${SONAR_RUNSYSTEMS_TOKEN:-}" ;;
    primeit) printf '%s' "${SONAR_PRIMEIT_TOKEN:-}" ;;
    *) die "unknown Sonar server: $1" ;;
  esac
}

urlencode() {
  printf '%s' "$1" | "$JQ_BIN" -sRr @uri
}

run_dir() {
  [[ -n "$RUN_ID" ]] || die "run id is required"
  [[ "$RUN_ID" =~ ^[A-Za-z0-9._-]+$ && "$RUN_ID" != . && "$RUN_ID" != .. ]] || die "invalid run id: $RUN_ID"
  printf '%s/runs/%s' "$STATE_DIR" "$RUN_ID"
}

ensure_state_dirs() {
  mkdir -p "$STATE_DIR/runs"
  chmod 700 "$STATE_DIR" "$STATE_DIR/runs"
}

ensure_run_dir() {
  local dir=$1
  mkdir -p "$dir"
  chmod 700 "$dir"
}

new_run_id() {
  date -u '+%Y%m%dT%H%M%SZ'
}

issue_marker() {
  "$JQ_BIN" -r '"bsl-sonar-triage-id: \(.server)/\(.project_key)/\(.issue_key)"'
}

ledger_has_marker() {
  local marker=$1
  [[ -f "$PROCESSED_LEDGER" ]] || return 1
  "$JQ_BIN" -e --arg m "$marker" -s 'any(.[]; .marker == $m)' "$PROCESSED_LEDGER" >/dev/null 2>&1
}

ledger_record() {
  local marker=$1
  local verdict=$2
  local run_id=$3
  "$JQ_BIN" -n -c --arg marker "$marker" --arg verdict "$verdict" --arg run "$run_id" \
    '{marker:$marker, verdict:$verdict, run_id:$run}' >> "$PROCESSED_LEDGER"
  chmod 600 "$PROCESSED_LEDGER" 2>/dev/null || true
}

sonar_get() {
  local server=$1
  local path=$2
  local query=$3
  local output=$4
  local token url
  token=$(server_token "$server")
  [[ -n "$token" ]] || die "missing token for Sonar server '$server'"
  url="$(server_url "$server")$path?$query"
  (
    local config
    config=$(mktemp)
    chmod 600 "$config"
    trap 'rm -f "$config"' EXIT
    printf 'header = "Authorization: Bearer %s"\n' "$token" > "$config"
    "$CURL_BIN" -fsS --config "$config" "$url" -o "$output"
  )
}

glab_issue_list() {
  local args=()
  if [[ -n "$TRIAGE_GITLAB_REPO" ]]; then
    args+=(--repo "$TRIAGE_GITLAB_REPO")
  fi
  "$GLAB_BIN" issue list --all --label "$TRIAGE_LABEL" --output json "${args[@]}"
}

preflight() {
  need_cmd "$CURL_BIN"
  need_cmd "$GLAB_BIN"
  need_cmd "$JQ_BIN"
  need_cmd "$OPENCODE_BIN"
  [[ -f "$PROMPT_FILE" ]] || die "prompt file not found: $PROMPT_FILE"
  "$GLAB_BIN" auth status >/dev/null 2>&1
  load_env
  [[ -n "${SONAR_RUNSYSTEMS_TOKEN:-}" || -n "${SONAR_PRIMEIT_TOKEN:-}" ]] || die "no Sonar token variables found"
  log "preflight ok; GitLab auth uses glab, Sonar tokens are loaded but not printed"
}

project_specs() {
  if [[ -n "$TRIAGE_PROJECTS" ]]; then
    tr ',' '\n' <<< "$TRIAGE_PROJECTS"
    return
  fi
  printf 'runsystems:*\nprimeit:*\n'
}
