default_low_analysis() {
  local rule=$1
  "$JQ_BIN" -n --arg rule "$rule" '
    def slug: ascii_downcase | gsub("[^a-z0-9]+"; "-") | gsub("^-+|-+$"; "");
    ($rule | sub("^bsl-analyzer:"; "") | slug) as $rule_slug
    | {
        confidence: "low", classification: "unknown",
        summary: "Контекст неполный; зафиксировать факт закрытия диагностики для будущего разбора.",
        evidence: [], unknowns: ["opencode analysis was skipped or returned invalid JSON"],
        problem_key: (if $rule_slug == "" then "unknown-problem" else $rule_slug end),
        problem_title: (if $rule == "" then "Разбор закрытого замечания" else $rule end),
        recommended_gitlab_action: "create_issue"
      }'
}

# When opencode gives no usable problem_key, we fall back to the rule slug. This
# is intentionally lossy: two distinct defects under the same Sonar rule then
# merge into one problem group. A model-provided key is what keeps them apart.
normalize_analysis() {
  local raw=$1
  local rule=$2
  "$JQ_BIN" -c --arg rule "$rule" '
    def slug: ascii_downcase | gsub("[^a-z0-9]+"; "-") | gsub("^-+|-+$"; "");
    (if type == "object" then . else {} end) as $in
    | (($in.confidence // "") | tostring) as $confidence
    | (($in.classification // "") | tostring) as $classification
    | (($in.recommended_gitlab_action // "") | tostring) as $action
    | (($in.problem_key // "") | tostring | slug) as $key
    | ($rule | sub("^bsl-analyzer:"; "") | slug) as $rule_slug
    | {
        confidence: (if (["high", "medium", "low"] | index($confidence)) then $confidence else "low" end),
        classification: (if (["valid_false_positive", "analyzer_gap", "stale_issue", "duplicate", "unknown"] | index($classification)) then $classification else "unknown" end),
        summary: (($in.summary // "Контекст неполный; зафиксировать факт закрытия диагностики для будущего разбора.") | tostring),
        evidence: (if ($in.evidence | type) == "array" then ($in.evidence | map(tostring)) else [] end),
        unknowns: (if ($in.unknowns | type) == "array" then ($in.unknowns | map(tostring)) else ["opencode analysis was skipped or returned incomplete JSON"] end),
        problem_key: (if $key == "" then (if $rule_slug == "" then "unknown-problem" else $rule_slug end) else $key end),
        problem_title: (($in.problem_title // (if $rule == "" then "Разбор закрытого замечания" else $rule end)) | tostring),
        recommended_gitlab_action: (if $action == "skip" then "skip" else "create_issue" end)
      }
  ' "$raw"
}

extract_json_object() {
  local raw=$1
  local rule=$2
  if "$JQ_BIN" -e '.' "$raw" >/dev/null 2>&1; then
    normalize_analysis "$raw" "$rule"
    return
  fi
  default_low_analysis "$rule"
}

run_opencode_safely() (
  local prompt=$1
  local issue_file=$2
  local name
  cd "$REPO_ROOT"
  while IFS= read -r name; do
    [[ "$name" == SONAR*TOKEN* ]] && unset "$name"
  done < <(compgen -v)
  "$OPENCODE_BIN" run --dir "$REPO_ROOT" "$prompt" --file "$issue_file" </dev/null
)

analyze_issue() {
  local issue=$1
  local dir=$2
  local idx=$3
  local known_groups=$4
  local issue_file raw prompt rule
  rule=$("$JQ_BIN" -r '.rule_key' <<< "$issue")
  issue_file="$dir/opencode-input-$idx.json"
  "$JQ_BIN" -n --argjson issue "$issue" --arg repo_root "$REPO_ROOT" --slurpfile groups "$known_groups" \
    '{task:"triage_closed_sonar_issue", repo_root:$repo_root, sonar_issue:$issue, existing_problems:($groups[0] // [])}' > "$issue_file"
  if [[ "$TRIAGE_SKIP_OPENCODE" == 1 ]]; then
    default_low_analysis "$rule"
    return
  fi
  raw="$dir/opencode-output-$idx.txt"
  prompt=$(cat "$PROMPT_FILE")
  if ! run_opencode_safely "$prompt" "$issue_file" > "$raw"; then
    default_low_analysis "$rule"
    return
  fi
  extract_json_object "$raw" "$rule"
}

seed_known_groups() {
  if [[ -f "$PROCESSED_LEDGER" ]]; then
    "$JQ_BIN" -s '[.[] | select(.problem_key != null and .problem_key != "")]
      | group_by(.problem_key)
      | map({key: .[0].problem_key, title: (.[0].problem_title // .[0].problem_key)})' "$PROCESSED_LEDGER"
  else
    printf '[]\n'
  fi
}

remember_group() {
  local known_groups=$1
  local key=$2
  local title=$3
  [[ -n "$key" ]] || return 0
  "$JQ_BIN" -e --arg k "$key" 'any(.[]; .key == $k)' "$known_groups" >/dev/null 2>&1 && return 0
  local tmp
  tmp=$(mktemp)
  "$JQ_BIN" --arg k "$key" --arg t "$title" '. + [{key:$k, title:$t}]' "$known_groups" > "$tmp"
  mv "$tmp" "$known_groups"
}

analyze() {
  local dir issue parsed idx manifest_tmp known_groups key title action
  dir=$(run_dir)
  [[ -f "$dir/issues.json" ]] || die "issues.json not found for run $RUN_ID"
  known_groups="$dir/known-groups.json"
  seed_known_groups > "$known_groups"
  : > "$dir/analysis.ndjson"
  idx=0
  while IFS= read -r issue; do
    parsed=$(analyze_issue "$issue" "$dir" "$idx" "$known_groups")
    action=$("$JQ_BIN" -r '.recommended_gitlab_action' <<< "$parsed")
    if [[ "$action" != skip ]]; then
      key=$("$JQ_BIN" -r '.problem_key' <<< "$parsed")
      title=$("$JQ_BIN" -r '.problem_title' <<< "$parsed")
      remember_group "$known_groups" "$key" "$title"
    fi
    "$JQ_BIN" -n -c --argjson issue "$issue" --argjson analysis "$parsed" '{issue:$issue, analysis:$analysis}' >> "$dir/analysis.ndjson"
    idx=$((idx + 1))
  done < <("$JQ_BIN" -c '.[]' "$dir/issues.json")
  "$JQ_BIN" -s '.' "$dir/analysis.ndjson" > "$dir/analysis.json"
  manifest_tmp="$dir/manifest.json.tmp"
  "$JQ_BIN" --argjson analyzed_count "$idx" '.stage="analyzed" | .analyzed_count=$analyzed_count' "$dir/manifest.json" > "$manifest_tmp"
  mv "$manifest_tmp" "$dir/manifest.json"
  log "analysis report: $dir/analysis.json"
}
