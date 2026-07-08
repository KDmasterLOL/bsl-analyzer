labels_for_confidence() {
  case "$1" in
    high) printf '%s,context-high' "$TRIAGE_LABEL" ;;
    medium) printf '%s,context-medium' "$TRIAGE_LABEL" ;;
    low) printf '%s,context-low,needs-investigation' "$TRIAGE_LABEL" ;;
    *) printf '%s,context-low,needs-investigation' "$TRIAGE_LABEL" ;;
  esac
}

problem_marker_for() {
  printf 'bsl-sonar-problem: %s' "$1"
}

# Body of a freshly created problem issue: description of the analyzer problem
# plus every collected example. members = JSON array of {issue, analysis}.
group_body() {
  local key=$1
  local title=$2
  local members=$3
  "$JQ_BIN" -rn --arg key "$key" --arg title "$title" --argjson members "$members" '
    ($members[0].analysis) as $repr
    | ($members[0].issue) as $ri
    | (
        [
          "<!-- bsl-sonar-problem: " + $key + " -->", "",
          "## Проблема", "",
          $title, "",
          "- **Rule:** `" + $ri.rule_key + "`",
          "- **Classification:** `" + $repr.classification + "`",
          "- **Confidence:** `" + $repr.confidence + "`", "",
          $repr.summary, "",
          "## Примеры (" + ($members | length | tostring) + ")", ""
        ]
        + ( [ $members | to_entries[] as $e
              | ($e.value.issue) as $is
              | (($is.snippet // "") | rtrimstr("\n")) as $snip
              | (
                  [
                    "### " + (($e.key + 1) | tostring) + ". `" + $is.component + "`:" + ($is.line | tostring) + "  (" + $is.server + "/" + $is.project_key + ")",
                    "",
                    "<!-- bsl-sonar-triage-id: " + $is.server + "/" + $is.project_key + "/" + $is.issue_key + " -->",
                    "",
                    "> " + ($is.message | gsub("\n"; " ")),
                    ""
                  ]
                  + (if $snip == "" then [] else ["```bsl", $snip, "```", ""] end)
                )
            ] | add )
      )
    | join("\n")'
}

# Body of a comment that appends one more example to an existing problem issue.
example_note_body() {
  local issue=$1
  "$JQ_BIN" -rn --argjson is "$issue" '
    (($is.snippet // "") | rtrimstr("\n")) as $snip
    | (
        [
          "Ещё пример этой проблемы (закрыто как FALSE-POSITIVE):", "",
          "<!-- bsl-sonar-triage-id: " + $is.server + "/" + $is.project_key + "/" + $is.issue_key + " -->",
          "",
          "**" + $is.server + "/" + $is.project_key + "** `" + $is.component + "`:" + ($is.line | tostring),
          "",
          "> " + ($is.message | gsub("\n"; " ")),
          ""
        ]
        + (if $snip == "" then [] else ["```bsl", $snip, "```"] end)
      )
    | join("\n")'
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

existing_gitlab_iid_for_problem() {
  local cache=$1
  local key=$2
  "$JQ_BIN" -r --arg m "bsl-sonar-problem: $key" '
    map(select((((.description // .body) // "")) | contains($m))) | (.[0].iid // empty)
  ' "$cache"
}

plan_gitlab() {
  local dir existing_cache key members iid confidence labels title member marker note_body body markers status
  dir=$(run_dir)
  [[ -f "$dir/analysis.json" ]] || die "analysis.json not found for run $RUN_ID"
  load_existing_gitlab_issues "$dir"
  existing_cache="$dir/gitlab-issues.json"
  : > "$dir/actions.ndjson"
  while IFS= read -r key; do
    [[ -n "$key" ]] || continue
    members=$("$JQ_BIN" -c --arg k "$key" '[.[] | select(.analysis.recommended_gitlab_action != "skip" and .analysis.problem_key == $k)]' "$dir/analysis.json")
    title=$("$JQ_BIN" -r '.[0].analysis.problem_title' <<< "$members")
    confidence=$("$JQ_BIN" -r '.[0].analysis.confidence' <<< "$members")
    labels=$(labels_for_confidence "$confidence")
    iid=""
    set +e
    iid=$(ledger_iid_for_key "$key")
    set -e
    [[ -z "$iid" ]] && iid=$(existing_gitlab_iid_for_problem "$existing_cache" "$key")
    if [[ -n "$iid" ]]; then
      while IFS= read -r member; do
        [[ -n "$member" ]] || continue
        marker=$(issue_marker <<< "$("$JQ_BIN" -c '.issue' <<< "$member")")
        ledger_has_marker "$marker" && continue
        [[ -n "$(existing_issue_for_marker "$existing_cache" "$marker")" ]] && continue
        note_body=$(example_note_body "$("$JQ_BIN" -c '.issue' <<< "$member")")
        "$JQ_BIN" -n -c --argjson iid "$iid" --arg body "$note_body" --arg key "$key" --arg title "$title" --arg marker "$marker" \
          '{kind:"note", iid:$iid, body:$body, problem_key:$key, problem_title:$title, marker:$marker}' >> "$dir/actions.ndjson"
      done < <("$JQ_BIN" -c '.[]' <<< "$members")
    else
      body=$(group_body "$key" "$title" "$members")
      markers=$("$JQ_BIN" -c '[.[].issue | "bsl-sonar-triage-id: \(.server)/\(.project_key)/\(.issue_key)"]' <<< "$members")
      "$JQ_BIN" -n -c --arg title "$title" --arg body "$body" --arg labels "$labels" --arg key "$key" --argjson markers "$markers" \
        '{kind:"create", title:("[sonar-triage] " + $title), body:$body, labels:$labels, problem_key:$key, problem_title:$title, markers:$markers}' >> "$dir/actions.ndjson"
    fi
  done < <("$JQ_BIN" -r '[.[] | select(.analysis.recommended_gitlab_action != "skip") | .analysis.problem_key] | unique[]' "$dir/analysis.json")
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
    "$JQ_BIN" -r '.[] | if .kind == "create"
      then "- create [" + .problem_key + "] " + .problem_title + " (" + ((.markers | length) | tostring) + " examples)"
      else "- note -> #" + (.iid | tostring) + " [" + .problem_key + "] " + .marker end' "$dir/actions.json"
  } > "$report"
  printf '%s\n' "$report"
}

gitlab_create_issue() {
  local title=$1
  local body=$2
  local labels=$3
  local args=()
  [[ -n "$TRIAGE_GITLAB_REPO" ]] && args+=(--repo "$TRIAGE_GITLAB_REPO")
  "$GLAB_BIN" issue create "${args[@]}" --title "$title" --description "$body" --label "$labels" --yes </dev/null
}

gitlab_note_issue() {
  local iid=$1
  local body=$2
  local args=()
  [[ -n "$TRIAGE_GITLAB_REPO" ]] && args+=(--repo "$TRIAGE_GITLAB_REPO")
  "$GLAB_BIN" issue note "$iid" "${args[@]}" --message "$body" </dev/null
}

apply_create_action() {
  local action=$1
  local key title body labels url iid marker
  key=$("$JQ_BIN" -r '.problem_key' <<< "$action")
  if ledger_has_created_problem "$key"; then
    log "skip already-created problem: $key"
    return 0
  fi
  title=$("$JQ_BIN" -r '.title' <<< "$action")
  body=$("$JQ_BIN" -r '.body' <<< "$action")
  labels=$("$JQ_BIN" -r '.labels' <<< "$action")
  url=$(gitlab_create_issue "$title" "$body" "$labels")
  # The issue is already created; never abort here on an unexpected output shape,
  # or a repeat apply would create a duplicate. A missing iid only disables note
  # append until the group is rediscovered via its bsl-sonar-problem marker.
  iid=$(printf '%s\n' "$url" | grep -oE '/issues/[0-9]+' | grep -oE '[0-9]+' | tail -1 || true)
  [[ -n "$iid" ]] || log "WARN: could not parse created issue iid from glab output: $url"
  while IFS= read -r marker; do
    [[ -n "$marker" ]] || continue
    ledger_record "$marker" create_issue "$key" "$("$JQ_BIN" -r '.problem_title' <<< "$action")" "$iid" "$RUN_ID"
  done < <("$JQ_BIN" -r '.markers[]' <<< "$action")
}

apply_note_action() {
  local action=$1
  local iid marker body key
  marker=$("$JQ_BIN" -r '.marker' <<< "$action")
  ledger_has_marker "$marker" && { log "skip already-applied example: $marker"; return 0; }
  iid=$("$JQ_BIN" -r '.iid' <<< "$action")
  body=$("$JQ_BIN" -r '.body' <<< "$action")
  key=$("$JQ_BIN" -r '.problem_key' <<< "$action")
  gitlab_note_issue "$iid" "$body"
  ledger_record "$marker" note "$key" "$("$JQ_BIN" -r '.problem_title' <<< "$action")" "$iid" "$RUN_ID"
}

record_processed_from_analysis() {
  local dir=$1
  local item marker verdict key title iid
  [[ -f "$dir/analysis.json" ]] || return 0
  while IFS= read -r item; do
    marker=$("$JQ_BIN" -r '.issue | "bsl-sonar-triage-id: \(.server)/\(.project_key)/\(.issue_key)"' <<< "$item")
    ledger_has_marker "$marker" && continue
    verdict=$("$JQ_BIN" -r '.analysis.recommended_gitlab_action' <<< "$item")
    key=$("$JQ_BIN" -r '.analysis.problem_key' <<< "$item")
    title=$("$JQ_BIN" -r '.analysis.problem_title' <<< "$item")
    # Inherit the group iid recorded by this run's create/note so members already
    # present in an existing issue still carry problem_key -> iid in the ledger.
    iid=""
    if [[ "$verdict" != skip ]]; then
      set +e
      iid=$(ledger_iid_for_key "$key")
      set -e
    fi
    ledger_record "$marker" "$verdict" "$key" "$title" "$iid" "$RUN_ID"
  done < <("$JQ_BIN" -c '.[]' "$dir/analysis.json")
}

apply_actions() {
  local live=$1
  local confirm=$2
  local dir action kind
  dir=$(run_dir)
  [[ -f "$dir/actions.json" ]] || die "actions.json not found for run $RUN_ID"
  if [[ "$live" != 1 ]]; then
    render_dry_run
    return
  fi
  [[ "$confirm" == 1 ]] || die "live apply requires --confirm-live"
  "$GLAB_BIN" auth status >/dev/null 2>&1
  while IFS= read -r action; do
    kind=$("$JQ_BIN" -r '.kind' <<< "$action")
    case "$kind" in
      create) apply_create_action "$action" ;;
      note) apply_note_action "$action" ;;
      *) die "unsupported action kind: $kind" ;;
    esac
  done < <("$JQ_BIN" -c '.[]' "$dir/actions.json")
  record_processed_from_analysis "$dir"
  "$JQ_BIN" -n --arg run_id "$RUN_ID" --arg completed_at "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '{run_id:$run_id, completed_at:$completed_at}' > "$STATE_DIR/last-success.json"
  log "live apply completed; state advanced"
}
