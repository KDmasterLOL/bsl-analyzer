#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
TMP=$(mktemp -d)
if [[ "${KEEP_TRIAGE_TEST_TMP:-0}" == 1 ]]; then
  echo "keeping temp dir: $TMP" >&2
else
  trap 'rm -rf "$TMP"' EXIT
fi

BIN_DIR="$TMP/bin"
STATE_DIR="$TMP/state"
ENV_FILE="$TMP/env"
LOG_FILE="$TMP/glab.log"
mkdir -p "$BIN_DIR" "$STATE_DIR"
touch "$LOG_FILE"
export TRIAGE_TEST_LOG_FILE="$LOG_FILE"

cat > "$ENV_FILE" <<'EOF'
SONAR_RUNSYSTEMS_URL=https://sonar.runsystems.test
SONAR_RUNSYSTEMS_TOKEN=secret-runsystems-token
SONAR_PRIMEIT_URL=https://sonar.primeit.test
SONAR_PRIMEIT_TOKEN=secret-primeit-token
EOF

cat > "$BIN_DIR/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
out=""
url=""
config=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -o) out=$2; shift 2 ;;
    --config) config=$2; shift 2 ;;
    -H) shift 2 ;;
    -*) shift ;;
    *) url=$1; shift ;;
  esac
done
[[ -n "$out" ]] || exit 2
if [[ -n "$config" && -n "${TRIAGE_TEST_LOG_FILE:-}" ]]; then
  if grep -q 'inherited-should-not-use' "$config"; then
    printf 'INHERITED TOKEN USED\n' >> "$TRIAGE_TEST_LOG_FILE"
  fi
fi
case "$url" in
  *api/projects/search*)
    cat > "$out" <<'JSON'
{"paging":{"pageIndex":1,"pageSize":500,"total":1},"components":[{"key":"bsl-analyzer-fixture","name":"BSL Analyzer Fixture"}]}
JSON
    ;;
  *api/issues/search*)
    cat > "$out" <<'JSON'
{"paging":{"pageIndex":1,"pageSize":500,"total":3},"issues":[{"key":"ISSUE-1","rule":"bsl:UnusedParameters","status":"RESOLVED","resolution":"FALSE-POSITIVE","message":"Unused parameter Element in event handler","component":"bsl-analyzer-fixture:src/CommonModules/M.bsl","line":7,"severity":"MAJOR","type":"CODE_SMELL","creationDate":"2026-01-01T00:00:00+0000","updateDate":"2026-01-02T00:00:00+0000","closeDate":"2026-01-02T00:00:00+0000"},{"key":"ISSUE-2","rule":"bsl:UnusedParameters","status":"RESOLVED","resolution":"FALSE-POSITIVE","message":"Unused parameter Command in event handler","component":"bsl-analyzer-fixture:src/CommonModules/N.bsl","line":11,"severity":"MINOR","type":"CODE_SMELL","creationDate":"2026-01-01T00:00:00+0000","updateDate":"2026-01-02T00:00:00+0000","closeDate":"2026-01-02T00:00:00+0000"},{"key":"ISSUE-3","rule":"bsl:OnlyCreationDate","status":"RESOLVED","resolution":"WONTFIX","message":"Creation date alone should not pass since filter","component":"bsl-analyzer-fixture:src/CommonModules/O.bsl","line":13,"severity":"MINOR","type":"CODE_SMELL","creationDate":"2026-01-03T00:00:00+0000","updateDate":"","closeDate":""}]}
JSON
    ;;
  *api/sources/issue_snippets*)
    cat > "$out" <<'JSON'
{"sources":[{"code":"Процедура Тест()"},{"code":"    Значение = 42;"},{"code":"КонецПроцедуры"}]}
JSON
    ;;
  *) echo '{}' > "$out" ;;
esac
EOF
chmod +x "$BIN_DIR/curl"

cat > "$BIN_DIR/glab" <<EOF
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "\$*" >> "$LOG_FILE"
if [[ "\$*" == auth\ status* ]]; then exit 0; fi
if [[ "\$*" == issue\ list* ]]; then
  printf '[]\n'
  exit 0
fi
if [[ "\$*" == issue\ create* ]]; then
  cat >/dev/null
  printf 'WRITE %s\n' "\$*" >> "$LOG_FILE"
  printf 'http://gitlab.test/fixture/bsl-analyzer/-/issues/100\n'
  exit 0
fi
if [[ "\$*" == issue\ note* ]]; then
  printf 'WRITE %s\n' "\$*" >> "$LOG_FILE"
  exit 0
fi
exit 0
EOF
chmod +x "$BIN_DIR/glab"

cat > "$BIN_DIR/opencode" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
cat >/dev/null
if env | grep -q '^SONAR_.*TOKEN='; then
  printf 'TOKEN ENV LEAK\n' >> "${TRIAGE_TEST_LOG_FILE:?}"
fi
cat <<'JSON'
{"confidence":"low","classification":"analyzer_gap","summary":"Недостаточно контекста, фиксируем сигнал.","evidence":["fixture opencode"],"unknowns":["нет полного downstream проекта"],"problem_key":"unused-parameters-event-handler","problem_title":"UnusedParameters на обработчике события","recommended_gitlab_action":"create_issue"}
JSON
EOF
chmod +x "$BIN_DIR/opencode"

export PATH="$BIN_DIR:$PATH"
export CURL_BIN="$BIN_DIR/curl"
export GLAB_BIN="$BIN_DIR/glab"
export OPENCODE_BIN="$BIN_DIR/opencode"
export TRIAGE_ENV_FILE="$ENV_FILE"
export TRIAGE_STATE_DIR="$STATE_DIR"
export TRIAGE_REPO_ROOT="$ROOT"
export TRIAGE_PROJECTS="runsystems:bsl-analyzer-fixture"
export TRIAGE_MAX_ISSUES=0
export SONAR_RUNSYSTEMS_TOKEN=inherited-should-not-use
export SONAR_TOKEN=generic-should-not-leak

"$ROOT/scripts/sonar-triage.sh" preflight

if "$ROOT/scripts/sonar-triage.sh" apply --run-id ../bad >/dev/null 2>&1; then
  echo "invalid run id was accepted" >&2
  exit 1
fi

# --- opencode path passes the problem_key through analysis (before ledger writes) ---
TRIAGE_SKIP_OPENCODE=0 "$ROOT/scripts/sonar-triage.sh" collect --run-id opencode-run --since 2026-01-01
TRIAGE_SKIP_OPENCODE=0 "$ROOT/scripts/sonar-triage.sh" analyze --run-id opencode-run
key=$(jq -r '.[0].analysis.problem_key' "$STATE_DIR/runs/opencode-run/analysis.json")
[[ "$key" == "unused-parameters-event-handler" ]] || { echo "expected opencode problem_key, got $key" >&2; exit 1; }

# --- Grouping: two same-rule false positives collapse into one issue ---
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" run-dry --run-id fixture-run --since 2026-01-01

RUN_DIR="$STATE_DIR/runs/fixture-run"
[[ -f "$RUN_DIR/issues.json" && -f "$RUN_DIR/analysis.json" && -f "$RUN_DIR/actions.json" && -f "$RUN_DIR/dry-run.md" ]]

collected=$(jq 'length' "$RUN_DIR/issues.json")
[[ "$collected" == 2 ]] || { echo "expected two collected issues, got $collected" >&2; exit 1; }

actions=$(jq 'length' "$RUN_DIR/actions.json")
[[ "$actions" == 1 ]] || { echo "expected one grouped action, got $actions" >&2; exit 1; }

kind=$(jq -r '.[0].kind' "$RUN_DIR/actions.json")
[[ "$kind" == create ]] || { echo "expected a create action, got $kind" >&2; exit 1; }

examples=$(jq '.[0].markers | length' "$RUN_DIR/actions.json")
[[ "$examples" == 2 ]] || { echo "expected two grouped examples, got $examples" >&2; exit 1; }

if grep -q 'secret-runsystems-token\|secret-primeit-token' "$RUN_DIR/dry-run.md" "$LOG_FILE"; then
  echo "token leaked into dry-run artifacts" >&2
  exit 1
fi
if grep -q '^WRITE ' "$LOG_FILE"; then
  echo "dry-run executed a GitLab write" >&2
  exit 1
fi

# --- Live create records both example markers under one problem/iid ---
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" collect --run-id live1 --since 2026-01-01
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" analyze --run-id live1
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" plan --run-id live1
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" apply --run-id live1 --live --confirm-live

creates=$(grep -c '^WRITE issue create' "$LOG_FILE")
[[ "$creates" == 1 ]] || { echo "expected one grouped create write, got $creates" >&2; exit 1; }

ledger_entries=$(wc -l < "$STATE_DIR/processed.ndjson")
[[ "$ledger_entries" == 2 ]] || { echo "expected two ledger entries after grouped create, got $ledger_entries" >&2; exit 1; }
distinct_keys=$(jq -r '.problem_key' "$STATE_DIR/processed.ndjson" | sort -u | wc -l)
[[ "$distinct_keys" == 1 ]] || { echo "expected both examples under one problem_key, got $distinct_keys" >&2; exit 1; }
iids=$(jq -r '.gitlab_iid' "$STATE_DIR/processed.ndjson" | sort -u | tr '\n' ' ')
[[ "$iids" == "100 " ]] || { echo "expected both examples to carry iid 100, got '$iids'" >&2; exit 1; }

# --- Ledger dedup: the same closures are not re-collected next run ---
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" collect --run-id live2 --since 2026-01-01
after=$(jq 'length' "$STATE_DIR/runs/live2/issues.json")
[[ "$after" == 0 ]] || { echo "expected zero issues after ledger dedup, got $after" >&2; exit 1; }
skipped=$(jq '.skipped_processed' "$STATE_DIR/runs/live2/manifest.json")
[[ "$skipped" == 2 ]] || { echo "expected two skipped-processed, got $skipped" >&2; exit 1; }

# --- Idempotency: re-applying the same run creates nothing new ---
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" apply --run-id live1 --live --confirm-live
creates_again=$(grep -c '^WRITE issue create' "$LOG_FILE")
[[ "$creates_again" == 1 ]] || { echo "repeat apply created a duplicate, creates=$creates_again" >&2; exit 1; }

# --- Note path: create + append-example both execute on a manual plan ---
NOTE_DIR="$STATE_DIR/runs/note-run"
mkdir -p "$NOTE_DIR"
cat > "$NOTE_DIR/actions.json" <<'JSON'
[
  {"kind":"create","title":"[sonar-triage] one","body":"body one","labels":"sonar-triage","problem_key":"manual-create-key","problem_title":"one","markers":["bsl-sonar-triage-id: s/p/CREATE-1"]},
  {"kind":"note","iid":200,"body":"body note","problem_key":"manual-note-key","problem_title":"two","marker":"bsl-sonar-triage-id: s/p/NOTE-1"}
]
JSON
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" apply --run-id note-run --live --confirm-live
creates_after_note=$(grep -c '^WRITE issue create' "$LOG_FILE")
[[ "$creates_after_note" == 2 ]] || { echo "expected a second create from manual plan, got $creates_after_note" >&2; exit 1; }
notes=$(grep -c '^WRITE issue note' "$LOG_FILE")
[[ "$notes" == 1 ]] || { echo "expected one appended example note, got $notes" >&2; exit 1; }

if grep -q 'TOKEN ENV LEAK\|INHERITED TOKEN USED' "$LOG_FILE"; then
  echo "token leaked to child process or inherited env token was used" >&2
  exit 1
fi

echo "offline sonar-triage test passed"
