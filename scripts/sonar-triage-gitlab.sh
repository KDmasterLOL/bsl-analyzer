labels_for_confidence() {
  case "$1" in
    high) printf '%s,context-high' "$TRIAGE_LABEL" ;;
    medium) printf '%s,context-medium' "$TRIAGE_LABEL" ;;
    low) printf '%s,context-low,needs-investigation' "$TRIAGE_LABEL" ;;
    *) printf '%s,context-low,needs-investigation' "$TRIAGE_LABEL" ;;
  esac
}

issue_title() {
  "$JQ_BIN" -r '"[sonar-triage] \(.rule_key): \(.message | gsub("\\n"; " ") | .[0:100])"'
}

issue_body() {
  local issue=$1
  local analysis=$2
  local marker=$3
  "$JQ_BIN" -n --argjson issue "$issue" --argjson analysis "$analysis" --arg marker "$marker" '
    [
      $marker, "", "## Что произошло",
      ("Sonar issue закрыт программистом: `" + $issue.issue_key + "`."),
      ("Rule: `" + $issue.rule_key + "`, status: `" + $issue.status + "`, resolution: `" + $issue.resolution + "`."),
      "", "## Оценка opencode", ("Confidence: `" + $analysis.confidence + "`."),
      ("Classification: `" + $analysis.classification + "`."), $analysis.summary,
      "", "## Контекст Sonar", ("Project: `" + $issue.server + "/" + $issue.project_key + "`."),
      ("Component: `" + $issue.component + "`, line: `" + ($issue.line|tostring) + "`."),
      "", "```bsl", ($issue.snippet // ""), "```", "", "## Что неизвестно",
      (($analysis.unknowns // []) | map("- " + .) | join("\n"))
    ] | join("\n")'
}

load_existing_gitlab_issues() {
  local dir=$1
  glab_issue_list > "$dir/gitlab-issues.json"
}

existing_issue_for_marker() {
  local cache=$1
  local marker=$2
  "$JQ_BIN" -c --arg marker "$marker" '
    map(select(([
      (.title // ""),
      (.description // ""),
      (.body // "")
    ] | join("\n")) | contains($marker))) | .[0] // empty
  ' "$cache"
}

planned_action_for_item() {
  local item=$1
  local existing_cache=$2
  local issue analysis recommended marker existing confidence labels title body
  issue=$("$JQ_BIN" -c '.issue' <<< "$item")
  analysis=$("$JQ_BIN" -c '.analysis' <<< "$item")
  recommended=$("$JQ_BIN" -r '.recommended_gitlab_action' <<< "$analysis")
  [[ "$recommended" == skip ]] && return 2
  marker=$(issue_marker <<< "$issue")
  existing=$(existing_issue_for_marker "$existing_cache" "$marker")
  [[ -n "$existing" ]] && return 2
  confidence=$("$JQ_BIN" -r '.confidence' <<< "$analysis")
  labels=$(labels_for_confidence "$confidence")
  body=$(issue_body "$issue" "$analysis" "$marker")
  title=$(issue_title <<< "$issue")
  "$JQ_BIN" -n --arg kind create --arg title "$title" --arg body "$body" --arg labels "$labels" --arg marker "$marker" \
    '{kind:$kind, title:$title, body:$body, labels:$labels, marker:$marker}'
}

plan_gitlab() {
  local dir existing_cache item action
  dir=$(run_dir)
  [[ -f "$dir/analysis.json" ]] || die "analysis.json not found for run $RUN_ID"
  load_existing_gitlab_issues "$dir"
  existing_cache="$dir/gitlab-issues.json"
  : > "$dir/actions.ndjson"
  while IFS= read -r item; do
    set +e
    action=$(planned_action_for_item "$item" "$existing_cache")
    status=$?
    set -e
    case "$status" in
      0) ;;
      2) continue ;;
      *) return "$status" ;;
    esac
    "$JQ_BIN" -c '.' <<< "$action" >> "$dir/actions.ndjson"
  done < <("$JQ_BIN" -c '.[]' "$dir/analysis.json")
  "$JQ_BIN" -s '.' "$dir/actions.ndjson" > "$dir/actions.json"
  log "planned $("$JQ_BIN" 'length' "$dir/actions.json") GitLab action(s): $dir/actions.json"
}

render_dry_run() {
  local dir report
  dir=$(run_dir)
  report="$dir/dry-run.md"
  {
    printf '# Sonar triage dry-run %s\n\n' "$RUN_ID"
    printf 'GitLab writes disabled.\n\n'
    printf 'Issues collected: %s\n\n' "$("$JQ_BIN" 'length' "$dir/issues.json")"
    printf 'Planned GitLab actions: %s\n\n' "$("$JQ_BIN" 'length' "$dir/actions.json")"
    "$JQ_BIN" -r '.[] | "- " + .kind + ": " + .title + " (" + .marker + ")"' "$dir/actions.json"
  } > "$report"
  printf '%s\n' "$report"
}

apply_live_action() {
  local action=$1
  local kind args title body labels
  kind=$("$JQ_BIN" -r '.kind' <<< "$action")
  args=()
  [[ -n "$TRIAGE_GITLAB_REPO" ]] && args+=(--repo "$TRIAGE_GITLAB_REPO")
  case "$kind" in
    create)
      title=$("$JQ_BIN" -r '.title' <<< "$action")
      body=$("$JQ_BIN" -r '.body' <<< "$action")
      labels=$("$JQ_BIN" -r '.labels' <<< "$action")
      "$GLAB_BIN" issue create "${args[@]}" --title "$title" --description "$body" --label "$labels" --yes </dev/null
      ;;
    *) die "unsupported action kind: $kind" ;;
  esac
}

record_processed_from_analysis() {
  local dir=$1
  local item marker verdict
  [[ -f "$dir/analysis.json" ]] || return 0
  while IFS= read -r item; do
    marker=$("$JQ_BIN" -r '.issue | "bsl-sonar-triage-id: \(.server)/\(.project_key)/\(.issue_key)"' <<< "$item")
    verdict=$("$JQ_BIN" -r '.analysis.recommended_gitlab_action' <<< "$item")
    ledger_has_marker "$marker" || ledger_record "$marker" "$verdict" "$RUN_ID"
  done < <("$JQ_BIN" -c '.[]' "$dir/analysis.json")
}

apply_actions() {
  local live=$1
  local confirm=$2
  local dir action
  dir=$(run_dir)
  [[ -f "$dir/actions.json" ]] || die "actions.json not found for run $RUN_ID"
  if [[ "$live" != 1 ]]; then
    render_dry_run
    return
  fi
  [[ "$confirm" == 1 ]] || die "live apply requires --confirm-live"
  "$GLAB_BIN" auth status >/dev/null 2>&1
  local marker
  while IFS= read -r action; do
    marker=$("$JQ_BIN" -r '.marker' <<< "$action")
    if ledger_has_marker "$marker"; then
      log "skip already-applied action: $marker"
      continue
    fi
    apply_live_action "$action"
    ledger_record "$marker" create_issue "$RUN_ID"
  done < <("$JQ_BIN" -c '.[]' "$dir/actions.json")
  record_processed_from_analysis "$dir"
  "$JQ_BIN" -n --arg run_id "$RUN_ID" --arg completed_at "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '{run_id:$run_id, completed_at:$completed_at}' > "$STATE_DIR/last-success.json"
  log "live apply completed; state advanced"
}
