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

# Resolve a Sonar project key to a local clone directory. An explicit map wins;
# otherwise each TRIAGE_SRC_ROOTS entry is probed for a <root>/<project> dir.
local_root_for_project() {
  local project=$1
  local mapped root
  if [[ -f "$TRIAGE_PROJECT_MAP" ]]; then
    mapped=$(awk -F= -v k="$project" '$1 == k {print substr($0, length($1) + 2); exit}' "$TRIAGE_PROJECT_MAP")
    if [[ -n "$mapped" ]]; then
      mapped="${mapped/#\~/$HOME}"
      [[ -d "$mapped" ]] && { printf '%s' "$mapped"; return 0; }
    fi
  fi
  for root in $TRIAGE_SRC_ROOTS; do
    [[ -d "$root/$project" ]] && { printf '%s' "$root/$project"; return 0; }
  done
  return 1
}

# Locate the actual source file for a normalized issue inside its local clone.
# Emits a downstream_local object for the opencode input. The relative path comes
# from an external Sonar component, so it is constrained to stay inside the clone.
resolve_downstream_local() {
  local issue=$1
  local project component rel root abs
  project=$("$JQ_BIN" -r '.project_key' <<< "$issue")
  component=$("$JQ_BIN" -r '.component' <<< "$issue")
  if [[ "$component" == "$project:"* ]]; then
    rel="${component#"$project":}"
  else
    rel="${component#*:}"
  fi
  if [[ -z "$rel" || "$rel" == /* || "$rel" == ".."* || "$rel" == *"/.."* ]]; then
    "$JQ_BIN" -n --arg rel "$rel" '{available:false, rel:$rel, reason:"unsafe relative path"}'
    return
  fi
  if ! root=$(local_root_for_project "$project"); then
    "$JQ_BIN" -n '{available:false}'
    return
  fi
  abs="$root/$rel"
  if [[ -f "$abs" ]]; then
    "$JQ_BIN" -n --arg root "$root" --arg path "$abs" --arg rel "$rel" \
      '{available:true, repo_root:$root, path:$path, rel:$rel}'
  else
    "$JQ_BIN" -n --arg root "$root" --arg rel "$rel" \
      '{available:false, repo_root:$root, rel:$rel, reason:"file not found in clone"}'
  fi
}

ledger_has_marker() {
  local marker=$1
  [[ -f "$PROCESSED_LEDGER" ]] || return 1
  "$JQ_BIN" -e --arg m "$marker" -s 'any(.[]; .marker == $m)' "$PROCESSED_LEDGER" >/dev/null 2>&1
}

ledger_record() {
  local marker=$1
  local verdict=$2
  local problem_key=$3
  local problem_title=$4
  local gitlab_iid=$5
  local run_id=$6
  "$JQ_BIN" -n -c --arg marker "$marker" --arg verdict "$verdict" --arg key "$problem_key" \
    --arg title "$problem_title" --arg iid "$gitlab_iid" --arg run "$run_id" \
    '{marker:$marker, verdict:$verdict, problem_key:$key, problem_title:$title,
      gitlab_iid:(if $iid == "" then null else ($iid | tonumber? // $iid) end), run_id:$run}' >> "$PROCESSED_LEDGER"
  chmod 600 "$PROCESSED_LEDGER" 2>/dev/null || true
}

ledger_iid_for_key() {
  local key=$1
  [[ -f "$PROCESSED_LEDGER" ]] || return 1
  local iid
  iid=$("$JQ_BIN" -rs --arg k "$key" \
    '[.[] | select(.problem_key == $k and .gitlab_iid != null)]
     | if length == 0 then "" else (.[-1].gitlab_iid | tostring) end' "$PROCESSED_LEDGER")
  [[ -n "$iid" ]] || return 1
  printf '%s' "$iid"
}

# A problem group is "created" once any of its examples is recorded with the
# create verdict — independent of whether the issue iid was parsed. This is the
# idempotency guard against a duplicate create when a live apply is repeated.
ledger_has_created_problem() {
  local key=$1
  [[ -f "$PROCESSED_LEDGER" ]] || return 1
  "$JQ_BIN" -e -s --arg k "$key" 'any(.[]; .problem_key == $k and .verdict == "create_issue")' "$PROCESSED_LEDGER" >/dev/null 2>&1
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
