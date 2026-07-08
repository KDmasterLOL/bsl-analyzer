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
{"paging":{"pageIndex":1,"pageSize":500,"total":3},"issues":[{"key":"ISSUE-1","rule":"bsl:MagicNumber","status":"RESOLVED","resolution":"FALSE-POSITIVE","message":"Magic number in allowed context","component":"bsl-analyzer-fixture:src/CommonModules/M.bsl","line":7,"severity":"MAJOR","type":"CODE_SMELL","creationDate":"2026-01-01T00:00:00+0000","updateDate":"2026-01-02T00:00:00+0000","closeDate":"2026-01-02T00:00:00+0000"},{"key":"ISSUE-2","rule":"bsl:UnusedParameters","status":"RESOLVED","resolution":"WONTFIX","message":"Unused parameter is intentional","component":"bsl-analyzer-fixture:src/CommonModules/N.bsl","line":11,"severity":"MINOR","type":"CODE_SMELL","creationDate":"2026-01-01T00:00:00+0000","updateDate":"2026-01-02T00:00:00+0000","closeDate":"2026-01-02T00:00:00+0000"},{"key":"ISSUE-3","rule":"bsl:OnlyCreationDate","status":"RESOLVED","resolution":"WONTFIX","message":"Creation date alone should not pass since filter","component":"bsl-analyzer-fixture:src/CommonModules/O.bsl","line":13,"severity":"MINOR","type":"CODE_SMELL","creationDate":"2026-01-03T00:00:00+0000","updateDate":"","closeDate":""}]}
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
  if [[ "\$*" == *ISSUE-2* ]]; then
    printf '[]\n'
    exit 0
  fi
  printf '[{"iid":42,"title":"existing sonar triage","description":"bsl-sonar-triage-id: runsystems/bsl-analyzer-fixture/ISSUE-1"}]\n'
  exit 0
fi
if [[ "\$*" == issue\ create* || "\$*" == issue\ note* || "\$*" == issue\ update* ]]; then
  cat >/dev/null
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
{"confidence":"low","classification":"unknown","summary":"Недостаточно контекста, фиксируем сигнал.","evidence":["fixture opencode"],"unknowns":["нет полного downstream проекта"],"recommended_gitlab_action":"create_issue"}
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
export TRIAGE_MAX_ISSUES=3
export SONAR_RUNSYSTEMS_TOKEN=inherited-should-not-use
export SONAR_TOKEN=generic-should-not-leak

"$ROOT/scripts/sonar-triage.sh" preflight

if "$ROOT/scripts/sonar-triage.sh" apply --run-id ../bad >/dev/null 2>&1; then
  echo "invalid run id was accepted" >&2
  exit 1
fi

TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" run-dry --run-id fixture-run --since 2026-01-01

RUN_DIR="$STATE_DIR/runs/fixture-run"
[[ -f "$RUN_DIR/issues.json" ]]
[[ -f "$RUN_DIR/analysis.json" ]]
[[ -f "$RUN_DIR/actions.json" ]]
[[ -f "$RUN_DIR/dry-run.md" ]]

collected=$(jq 'length' "$RUN_DIR/issues.json")
[[ "$collected" == 2 ]] || { echo "expected two collected issues after close/update date filter, got $collected" >&2; exit 1; }

if grep -q 'secret-runsystems-token\|secret-primeit-token' "$RUN_DIR/dry-run.md" "$LOG_FILE"; then
  echo "token leaked into dry-run artifacts" >&2
  exit 1
fi

if grep -q '^WRITE ' "$LOG_FILE"; then
  echo "dry-run executed a GitLab write" >&2
  exit 1
fi

actions=$(jq 'length' "$RUN_DIR/actions.json")
[[ "$actions" == 1 ]] || { echo "expected one planned action, got $actions" >&2; exit 1; }

notes=$(jq '[.[] | select(.kind == "note")] | length' "$RUN_DIR/actions.json")
[[ "$notes" == 0 ]] || { echo "expected zero planned notes, got $notes" >&2; exit 1; }

creates=$(jq '[.[] | select(.kind == "create")] | length' "$RUN_DIR/actions.json")
[[ "$creates" == 1 ]] || { echo "expected one planned create, got $creates" >&2; exit 1; }

list_calls=$(grep -c '^issue list' "$LOG_FILE")
[[ "$list_calls" == 1 ]] || { echo "expected one GitLab issue list call, got $list_calls" >&2; exit 1; }

LIVE_RUN_DIR="$STATE_DIR/runs/live-loop-run"
mkdir -p "$LIVE_RUN_DIR"
cat > "$LIVE_RUN_DIR/actions.json" <<'JSON'
[
  {"kind":"create","title":"one","body":"body one","labels":"sonar-triage","marker":"m1"},
  {"kind":"create","title":"two","body":"body two","labels":"sonar-triage","marker":"m2"}
]
JSON
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" apply --run-id live-loop-run --live --confirm-live
write_calls=$(grep -c '^WRITE ' "$LOG_FILE")
[[ "$write_calls" == 2 ]] || { echo "expected two fake GitLab writes, got $write_calls" >&2; exit 1; }

TRIAGE_SKIP_OPENCODE=0 "$ROOT/scripts/sonar-triage.sh" collect --run-id opencode-run --since 2026-01-01
TRIAGE_SKIP_OPENCODE=0 "$ROOT/scripts/sonar-triage.sh" analyze --run-id opencode-run
analysis_count=$(jq 'length' "$STATE_DIR/runs/opencode-run/analysis.json")
[[ "$analysis_count" == 2 ]] || { echo "expected two analyzed issues, got $analysis_count" >&2; exit 1; }
confidence=$(jq -r '.[0].analysis.confidence' "$STATE_DIR/runs/opencode-run/analysis.json")
[[ "$confidence" == low ]] || { echo "expected fake opencode confidence low, got $confidence" >&2; exit 1; }

# A live apply records processed markers so the next collect drops them before analyze.
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" collect --run-id ledger-a --since 2026-01-01
before_ledger=$(jq 'length' "$STATE_DIR/runs/ledger-a/issues.json")
[[ "$before_ledger" == 2 ]] || { echo "expected two issues before ledger apply, got $before_ledger" >&2; exit 1; }
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" analyze --run-id ledger-a
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" plan --run-id ledger-a
TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" apply --run-id ledger-a --live --confirm-live
[[ -f "$STATE_DIR/processed.ndjson" ]] || { echo "ledger not written after live apply" >&2; exit 1; }
for k in ISSUE-1 ISSUE-2; do
  jq -e --arg k "$k" -s 'any(.[]; .marker | endswith("/" + $k))' "$STATE_DIR/processed.ndjson" >/dev/null \
    || { echo "ledger missing marker for $k after live apply" >&2; exit 1; }
done

TRIAGE_SKIP_OPENCODE=1 "$ROOT/scripts/sonar-triage.sh" collect --run-id ledger-b --since 2026-01-01
after_ledger=$(jq 'length' "$STATE_DIR/runs/ledger-b/issues.json")
[[ "$after_ledger" == 0 ]] || { echo "expected zero issues after ledger dedup, got $after_ledger" >&2; exit 1; }
skipped_processed=$(jq '.skipped_processed' "$STATE_DIR/runs/ledger-b/manifest.json")
[[ "$skipped_processed" == 2 ]] || { echo "expected two skipped-processed, got $skipped_processed" >&2; exit 1; }

if grep -q 'TOKEN ENV LEAK\|INHERITED TOKEN USED' "$LOG_FILE"; then
  echo "token leaked to child process or inherited env token was used" >&2
  exit 1
fi

echo "offline sonar-triage test passed"
