discover_projects_for_server() {
  local server=$1
  local out=$2
  local page=1
  local tmp total ps
  while :; do
    tmp=$(mktemp)
    if ! sonar_get "$server" /api/projects/search "ps=500&p=$page" "$tmp"; then rm -f "$tmp"; return 1; fi
    if ! "$JQ_BIN" -c --arg server "$server" '.components[]? | {server:$server, key:.key, name:(.name // .key)}' "$tmp" >> "$out"; then rm -f "$tmp"; return 1; fi
    if ! total=$("$JQ_BIN" -r '.paging.total // 0' "$tmp"); then rm -f "$tmp"; return 1; fi
    if ! ps=$("$JQ_BIN" -r '.paging.pageSize // 500' "$tmp"); then rm -f "$tmp"; return 1; fi
    rm -f "$tmp"
    (( page * ps >= total )) && break
    page=$((page + 1))
  done
}

discover() {
  load_env
  RUN_ID=${RUN_ID:-$(new_run_id)}
  local dir projects
  dir=$(run_dir)
  ensure_run_dir "$dir"
  projects="$dir/projects.ndjson"
  : > "$projects"
  discover_projects_for_server runsystems "$projects"
  discover_projects_for_server primeit "$projects"
  "$JQ_BIN" -s --arg run_id "$RUN_ID" '{run_id:$run_id, projects:.}' "$projects" > "$dir/discover.json"
  log "discover report: $dir/discover.json"
}

collect_project_keys() {
  local server=$1
  local wanted=$2
  local tmp
  if [[ "$wanted" != '*' ]]; then
    printf '%s\n' "$wanted"
    return
  fi
  tmp=$(mktemp)
  if ! discover_projects_for_server "$server" "$tmp"; then rm -f "$tmp"; return 1; fi
  if ! "$JQ_BIN" -r '.key' "$tmp"; then rm -f "$tmp"; return 1; fi
  rm -f "$tmp"
}

snippet_for_issue() {
  local server=$1
  local issue_key=$2
  local component=$3
  local line=${4:-0}
  local tmp from to query
  tmp=$(mktemp)
  if sonar_get "$server" /api/sources/issue_snippets "issueKey=$(urlencode "$issue_key")" "$tmp" 2>/dev/null; then
    if "$JQ_BIN" -e '.' "$tmp" >/dev/null 2>&1; then
      "$JQ_BIN" -r '[.. | objects | .code? // empty] | join("\n")' "$tmp" || true
      rm -f "$tmp"
      return
    fi
  fi
  if [[ "$line" =~ ^[0-9]+$ && "$line" -gt 0 ]]; then
    from=$((line > 10 ? line - 10 : 1))
    to=$((line + 10))
    query="key=$(urlencode "$component")&from=$from&to=$to"
    if sonar_get "$server" /api/sources/lines "$query" "$tmp" 2>/dev/null; then
      "$JQ_BIN" -r '.sources[]? | "\(.line): \(.code // "")"' "$tmp" || true
    fi
  fi
  rm -f "$tmp"
}

append_issue() {
  local normalized=$1
  local server=$2
  local issues=$3
  local snippet
  if [[ "$TRIAGE_INCLUDE_SNIPPETS" != 1 ]]; then
    snippet=""
  else
    snippet=$(snippet_for_issue "$server" "$("$JQ_BIN" -r '.issue_key' <<< "$normalized")" "$("$JQ_BIN" -r '.component' <<< "$normalized")" "$("$JQ_BIN" -r '.line // 0' <<< "$normalized")")
  fi
  "$JQ_BIN" -c --arg snippet "$snippet" '. + {snippet:$snippet}' <<< "$normalized" >> "$issues"
}

normalize_issues() {
  local server=$1
  local project=$2
  local tmp=$3
  "$JQ_BIN" -c --arg server "$server" --arg project "$project" --arg since "$TRIAGE_SINCE" '
    def first_nonempty(xs): first(xs[] | select(. != null and . != "")) // "";
    .issues[]? as $issue
    | (first_nonempty([$issue.closeDate, $issue.updateDate]) | .[0:10]) as $triage_date
    | select($triage_date != "" and $triage_date >= $since)
    | {
      server:$server, project_key:$project, issue_key:($issue.key // ""), rule_key:($issue.rule // ""),
      status:($issue.status // $issue.issueStatus // ""), resolution:($issue.resolution // ""), message:($issue.message // ""),
      component:($issue.component // ""), line:($issue.line // $issue.textRange.startLine // 0),
      severity:($issue.severity // ""), type:($issue.type // ""), creation_date:($issue.creationDate // ""),
      update_date:($issue.updateDate // ""), close_date:($issue.closeDate // ""), triage_date:$triage_date
    }' "$tmp"
}

collect_project_issues() {
  local server=$1
  local key=$2
  local issues=$3
  local count_ref=$4
  local skip_ref=$5
  local page=1
  local tmp query page_items total ps normalized marker
  while :; do
    tmp=$(mktemp)
    query="projects=$(urlencode "$key")&resolved=true&statuses=RESOLVED,CLOSED&resolutions=$(urlencode "$TRIAGE_RESOLUTIONS")&s=CLOSE_DATE&asc=false&additionalFields=rules&ps=500&p=$page"
    if ! sonar_get "$server" /api/issues/search "$query" "$tmp"; then rm -f "$tmp"; return 1; fi
    if ! page_items=$("$JQ_BIN" -r '.issues | length' "$tmp"); then rm -f "$tmp"; return 1; fi
    while IFS= read -r normalized; do
      [[ -n "$normalized" ]] || continue
      marker=$(printf '%s' "$normalized" | issue_marker)
      if ledger_has_marker "$marker"; then
        printf -v "$skip_ref" '%s' "$((${!skip_ref} + 1))"
        continue
      fi
      if ! append_issue "$normalized" "$server" "$issues"; then rm -f "$tmp"; return 1; fi
      printf -v "$count_ref" '%s' "$((${!count_ref} + 1))"
      if [[ "$TRIAGE_MAX_ISSUES" != 0 && "${!count_ref}" -ge "$TRIAGE_MAX_ISSUES" ]]; then
        rm -f "$tmp"
        return 2
      fi
    done < <(normalize_issues "$server" "$key" "$tmp")
    if ! total=$("$JQ_BIN" -r '.paging.total // 0' "$tmp"); then rm -f "$tmp"; return 1; fi
    if ! ps=$("$JQ_BIN" -r '.paging.pageSize // 500' "$tmp"); then rm -f "$tmp"; return 1; fi
    rm -f "$tmp"
    (( page_items == 0 || page * ps >= total )) && break
    page=$((page + 1))
  done
}

collect() {
  load_env
  RUN_ID=${RUN_ID:-$(new_run_id)}
  local dir issues count skipped spec server project_key key keys_file status
  dir=$(run_dir)
  ensure_run_dir "$dir"
  issues="$dir/issues.ndjson"
  : > "$issues"
  count=0
  skipped=0
  while IFS=: read -r server project_key; do
    [[ -n "$server" && -n "$project_key" ]] || continue
    keys_file=$(mktemp)
    if ! collect_project_keys "$server" "$project_key" > "$keys_file"; then rm -f "$keys_file"; return 1; fi
    while IFS= read -r key; do
      [[ -n "$key" ]] || continue
      set +e
      collect_project_issues "$server" "$key" "$issues" count skipped
      status=$?
      set -e
      case "$status" in
        0) ;;
        2) rm -f "$keys_file"; break 2 ;;
        *) rm -f "$keys_file"; return "$status" ;;
      esac
    done < "$keys_file"
    rm -f "$keys_file"
  done < <(project_specs)
  "$JQ_BIN" -s '.' "$issues" > "$dir/issues.json"
  "$JQ_BIN" -n --arg run_id "$RUN_ID" --arg since "$TRIAGE_SINCE" --argjson count "$count" --argjson skipped "$skipped" \
    '{run_id:$run_id, since:$since, issue_count:$count, skipped_processed:$skipped, dry_run:true, stage:"collected"}' > "$dir/manifest.json"
  log "collected $count issue(s), skipped $skipped already-processed: $dir/issues.json"
}
